use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    path::Path,
    str::FromStr,
};

use crate::World;
use anyhow::{bail, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use similar::DiffableStr;
use tempfile::tempdir;
use serde::de::Error;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum Diff {
    VersionAdded(String),
    VersionRemoved,
}

#[derive(PartialEq, Debug, PartialOrd, Ord, Eq)]
pub struct VersionRange(Option<Version>, Option<Version>);

impl Serialize for VersionRange {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let v0 = self.0.as_ref().map(|v| v.to_string()).unwrap_or_default();
        let v1 = self.1.as_ref().map(|v| v.to_string()).unwrap_or_default();
        serializer.serialize_str(&format!("{}...{}", v0, v1))
    }
}

impl<'de> Deserialize<'de> for VersionRange {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
            let s: String = Deserialize::deserialize(deserializer)?;
            let parts = s.split("...").collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(D::Error::custom("Fail to deserialize VersionRange, expected {from}...{to}"))
            }

            let v0 = (!parts[0].is_empty()).then(|| Version::from_str(parts[0]).map_err(D::Error::custom)).transpose()?;
            let v1 = (!parts[1].is_empty()).then(|| Version::from_str(parts[1]).map_err(D::Error::custom)).transpose()?;
            Ok(VersionRange(v0, v1))
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct CombinedDiff {
    pub world_name: String,
    pub diffs: BTreeMap<VersionRange, Diff>,
}

pub async fn diff_world_and_write(
    from: Option<&World>,
    to: Option<&World>,
    world_name: &str,
    destination: &Path,
    ap_index_url: &str,
    ap_index_ref: &str,
) -> Result<()> {
    let diff = diff_world(from, to, ap_index_url, ap_index_ref).await?;

    std::fs::create_dir_all(destination)?;
    let file_path = destination.join(format!("{}.apdiff", world_name));
    let mut out = Vec::new();
    let serializer = &mut serde_json::Serializer::new(&mut out);
    serde_path_to_error::serialize(&diff, serializer)?;
    std::fs::write(file_path, out)?;

    Ok(())
}

async fn diff_world(
    from: Option<&World>,
    to: Option<&World>,
    ap_index_url: &str,
    ap_index_ref: &str,
) -> Result<CombinedDiff> {
    match (from, to) {
        // World added
        (None, Some(new_world)) => {
            let mut previous_version = None;

            let mut result = CombinedDiff {
                world_name: new_world
                    .path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                diffs: BTreeMap::new(),
            };

            for version in new_world.versions.keys() {
                let diff = diff_version(
                    &new_world,
                    previous_version.clone(),
                    &new_world,
                    Some(version),
                    ap_index_url,
                    ap_index_ref,
                )
                .await?;
                result.diffs.insert(
                    VersionRange(previous_version, Some(version.clone())),
                    Diff::VersionAdded(diff),
                );

                previous_version = Some(version.clone());
            }

            Ok(result)
        }
        // World removed
        (Some(old_world), None) => {
            // We don't have anything to review for worlds removal, don't include diffs
            let mut result = CombinedDiff {
                world_name: old_world
                    .path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                diffs: BTreeMap::new(),
            };

            for version in old_world.versions.keys() {
                result.diffs.insert(
                    VersionRange(Some(version.clone()), None),
                    Diff::VersionRemoved,
                );
            }

            Ok(result)
        }
        // World changed
        (Some(old_world), Some(new_world)) => {
            //let mut previous_version = None;

            let mut result = CombinedDiff {
                world_name: new_world
                    .path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                diffs: BTreeMap::new(),
            };

            let mut candidate_versions = old_world.versions.keys().collect::<Vec<_>>();
            for version in new_world.versions.keys() {
                if old_world.versions.contains_key(version) {
                    continue;
                }

                let previous_version =
                    find_closest_version(version, old_world.versions.keys().collect());
                let origin_diff = diff_version(
                    &old_world,
                    previous_version.clone(),
                    new_world,
                    Some(version),
                    ap_index_url,
                    ap_index_ref,
                )
                .await?;
                result.diffs.insert(
                    VersionRange(previous_version, Some(version.clone())),
                    Diff::VersionAdded(origin_diff),
                );
                candidate_versions.push(version);
            }

            for version in old_world.versions.keys() {
                if new_world.versions.contains_key(version) {
                    continue;
                }

                result.diffs.insert(
                    VersionRange(Some(version.clone()), None),
                    Diff::VersionRemoved,
                );
            }

            Ok(result)
        }
        (None, None) => bail!("You can't diff a non existent world with another non existent one"),
    }
}

fn find_closest_version(target_version: &Version, mut versions: Vec<&Version>) -> Option<Version> {
    let mut candidate_version = None;
    versions.sort();

    for version in versions {
        if version < target_version {
            candidate_version = Some(version);
            continue;
        }

        break;
    }

    candidate_version.cloned()
}

async fn diff_version(
    from_world: &World,
    from_version: Option<Version>,
    to_world: &World,
    to_version: Option<&Version>,
    ap_index_url: &str,
    ap_index_ref: &str,
) -> Result<String> {
    let from_tmpdir = tempdir()?;
    let to_tmpdir = tempdir()?;

    if let Some(from_version) = from_version {
        from_world
            .extract_to(
                &from_version,
                from_tmpdir.path(),
                ap_index_url,
                ap_index_ref,
            )
            .await?;
    }
    if let Some(to_version) = to_version {
        to_world
            .extract_to(&to_version, to_tmpdir.path(), ap_index_url, ap_index_ref)
            .await?;
    }

    let diff = diff_dir(from_tmpdir.path(), to_tmpdir.path())?;

    Ok(diff)
}

pub fn diff_dir<'a>(from: &Path, to: &Path) -> Result<String> {
    let mut combined_paths = walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.file_type().is_dir())
        .map(|entry| entry.path().strip_prefix(from).unwrap().to_owned())
        .collect::<HashSet<_>>();

    combined_paths.extend(
        walkdir::WalkDir::new(to)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| !e.file_type().is_dir())
            .map(|entry| entry.path().strip_prefix(to).unwrap().to_owned()),
    );

    let mut result = "".to_string();
    for file in &combined_paths {
        let from_path = from.join(file);
        let to_path = to.join(file);

        let from_value = std::fs::read_to_string(&from_path).unwrap_or_else(|_| "".to_string());
        let to_value = std::fs::read_to_string(&to_path).unwrap_or_else(|_| "".to_string());
        let file_diff = similar::TextDiff::from_lines(&from_value, &to_value);
        let mut udiff = file_diff.unified_diff();

        let from = if from_path.is_file() {
            from_path.strip_prefix(from).unwrap().to_string_lossy()
        } else {
            Cow::from("/dev/null")
        };
        let to = if to_path.is_file() {
            to_path.strip_prefix(to).unwrap().to_string_lossy()
        } else {
            Cow::from("/dev/null")
        };

        udiff
            .header(from.as_ref(), to.as_ref())
            .missing_newline_hint(false);
        result += &format!("{}", udiff);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::str::FromStr;

    use crate::{World, WorldOrigin};
    use anyhow::Result;
    use semver::Version;
    use tempfile::{tempdir, TempDir};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::{CombinedDiff, Diff, VersionRange};

    use super::diff_world;

    fn get_mock_world_versions(
        versions: &[&str],
    ) -> Result<(TempDir, BTreeMap<Version, WorldOrigin>)> {
        let mut result = BTreeMap::new();
        let tmpdir = tempdir()?;

        for version in versions {
            let path = tmpdir.path().join(version);

            let apworld_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            let mut archive = ZipWriter::new(&apworld_file);
            archive.start_file("VERSION", SimpleFileOptions::default())?;
            archive.write_all(version.as_bytes())?;
            archive.finish()?;
            result.insert(
                Version::from_str(version)?,
                WorldOrigin::Local(path.clone()),
            );
        }
        Ok((tmpdir, result))
    }

    #[tokio::test]
    async fn test_add_world() -> Result<()> {
        let (_tmpdir, versions) = get_mock_world_versions(&["0.0.1", "0.0.3", "0.0.2"])?;

        let new_world = World {
            path: "/tmp/foobar.toml".into(),
            name: "New World".into(),
            display_name: "New World".into(),
            default_url: None,
            versions,
            home: None,
            disabled: false,
            supported: false,
        };

        let diff = diff_world(None, Some(&new_world), "", "").await?;

        let expected_diff = CombinedDiff {
            world_name: "foobar".to_string(),
            diffs: BTreeMap::from([
                (
                    VersionRange(None, Some(Version::from_str("0.0.1")?)),
                    Diff::VersionAdded(
                        "--- /dev/null\n+++ VERSION\n@@ -0,0 +1 @@\n+0.0.1\n".to_string(),
                    ),
                ),
                (
                    VersionRange(
                        Some(Version::from_str("0.0.1")?),
                        Some(Version::from_str("0.0.2")?),
                    ),
                    Diff::VersionAdded(
                        "--- VERSION\n+++ VERSION\n@@ -1 +1 @@\n-0.0.1\n+0.0.2\n".to_string(),
                    ),
                ),
                (
                    VersionRange(
                        Some(Version::from_str("0.0.2")?),
                        Some(Version::from_str("0.0.3")?),
                    ),
                    Diff::VersionAdded(
                        "--- VERSION\n+++ VERSION\n@@ -1 +1 @@\n-0.0.2\n+0.0.3\n".to_string(),
                    ),
                ),
            ]),
        };

        assert_eq!(diff, expected_diff);

        Ok(())
    }

    #[tokio::test]
    async fn test_remove_world() -> Result<()> {
        let (_tmpdir, versions) = get_mock_world_versions(&["0.0.1", "0.0.2"])?;

        let old_world = World {
            path: "/tmp/foobar.toml".into(),
            name: "Old World".into(),
            display_name: "Old World".into(),
            default_url: None,
            versions,
            home: None,
            disabled: false,
            supported: false,
        };

        let diff = diff_world(Some(&old_world), None, "", "").await?;

        let expected_diff = CombinedDiff {
            world_name: "foobar".to_string(),
            diffs: BTreeMap::from([
                (
                    VersionRange(Some(Version::from_str("0.0.1")?), None),
                    Diff::VersionRemoved,
                ),
                (
                    VersionRange(Some(Version::from_str("0.0.2")?), None),
                    Diff::VersionRemoved,
                ),
            ]),
        };

        assert_eq!(diff, expected_diff);

        Ok(())
    }

    #[tokio::test]
    async fn test_change_world() -> Result<()> {
        let (_tmpdir, old_versions) = get_mock_world_versions(&["0.0.1", "0.0.2", "0.0.3"])?;

        let old_world = World {
            path: "/tmp/foobar.toml".into(),
            name: "World".into(),
            display_name: "World".into(),
            default_url: None,
            versions: old_versions,
            home: None,
            disabled: false,
            supported: false,
        };

        let (_tmpdir, new_versions) = get_mock_world_versions(&["0.0.1", "0.0.3", "0.0.4"])?;

        let new_world = World {
            path: "/tmp/foobar.toml".into(),
            name: "World".into(),
            display_name: "World".into(),
            default_url: None,
            versions: new_versions,
            home: None,
            disabled: false,
            supported: false,
        };

        let diff = diff_world(Some(&old_world), Some(&new_world), "", "").await?;

        let expected_diff = CombinedDiff {
            world_name: "foobar".to_string(),
            diffs: BTreeMap::from([
                (
                    VersionRange(Some(Version::from_str("0.0.2")?), None),
                    Diff::VersionRemoved,
                ),
                (
                    VersionRange(
                        Some(Version::from_str("0.0.3")?),
                        Some(Version::from_str("0.0.4")?),
                    ),
                    Diff::VersionAdded(
                        "--- VERSION\n+++ VERSION\n@@ -1 +1 @@\n-0.0.3\n+0.0.4\n".to_string(),
                    ),
                ),
            ]),
        };

        assert_eq!(diff, expected_diff);

        Ok(())
    }
}