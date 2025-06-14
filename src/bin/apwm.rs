use anyhow::{bail, Context, Result};
use apwm::utils::git_clone_shallow;
use apwm::{diff::diff_world_and_write, WorldOrigin};
use clap::Parser;
use reqwest::Url;
use semver::Version;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::fs::File;
use std::io::Write;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[derive(clap::Subcommand)]
enum Command {
    Update {
        #[clap(short)]
        index_path: PathBuf,
    },
    Download {
        #[clap(short)]
        index_path: PathBuf,
        #[clap(short)]
        destination: PathBuf,
        #[clap(short)]
        precise: Option<String>,
    },
    Install {
        #[clap(short)]
        index_path: PathBuf,
        #[clap(short)]
        apworlds_path: PathBuf,
        #[clap(short)]
        destination: PathBuf,
        #[clap(short)]
        precise: Option<String>,
    },
    Diff {
        #[clap(short)]
        index_path: PathBuf,
        #[clap(short)]
        from: String,
        #[clap(short = 'r')]
        from_ref: Option<String>,
        #[clap(short)]
        output: PathBuf,
    },
}

#[derive(clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Args::parse();
    match cli.command {
        Command::Update { index_path } => {
            update(&index_path).await?;
        }
        Command::Download {
            index_path,
            destination,
            precise,
        } => {
            download(&index_path, &destination, &precise).await?;
        }
        Command::Install {
            index_path,
            apworlds_path,
            destination,
            precise,
        } => {
            install(&index_path, &apworlds_path, &destination, &precise).await?;
        }
        Command::Diff {
            index_path,
            from,
            from_ref,
            output,
        } => {
            let index_diff = diff(&index_path, &from, &from_ref, &output).await?;
            let serialized = serde_json::to_string(&index_diff)?;
            let output_path = output.join("apdiff.diff");
            let mut file = File::create(&output_path)?;
            file.write_all(serialized.as_bytes())?;
        }
    }

    Ok(())
}

async fn download(index_path: &Path, destination: &Path, precise: &Option<String>) -> Result<()> {
    let index_toml = index_path.join("index.toml");
    let index = apwm::Index::new(&index_toml)?;
    let target = apworld_version_from_precise(precise)?;

    index.refresh_into(destination, false, target).await?;

    Ok(())
}

async fn update(index_path: &Path) -> Result<()> {
    let index_toml = index_path.join("index.toml");
    let index = apwm::Index::new(&index_toml)?;
    let destination = tempdir()?;

    let new_lock = index.refresh_into(destination.path(), true, None).await?;

    new_lock.write()?;

    Ok(())
}

#[derive(Serialize)]
pub enum ApworldDiff {
    Added(Version, Option<String>),
    Removed(Version),
}

#[derive(Default)]
pub struct IndexDiff(HashMap<String, Vec<ApworldDiff>>);

impl Serialize for IndexDiff {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            if !value.is_empty() {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

async fn diff(
    index_path: &Path,
    from_git_remote: &str,
    from_git_ref: &Option<String>,
    output: &Path,
) -> Result<IndexDiff> {
    let old_index_dir = tempdir()?;
    git_clone_shallow(
        from_git_remote,
        from_git_ref.as_ref().map_or("main", |v| v),
        old_index_dir.path(),
    )?;

    let mut index_diff = IndexDiff::default();

    let new_index_toml = index_path.join("index.toml");
    let new_index = apwm::Index::new(&new_index_toml)?;

    // This will update the lockfile
    let new_index_lock = new_index.refresh_into(output, true, None).await?;
    new_index_lock.write()?;

    let old_index_toml = old_index_dir.path().join("index.toml");
    let old_index = apwm::Index::new(&old_index_toml)?;

    let old_worlds = old_index.worlds;
    let new_worlds = new_index.worlds;

    for (name, world) in &new_worlds {
        let mut versions = index_diff.0.entry(name.to_string()).or_default();

        match old_worlds.get(name) {
            // This is a new world, diff from nothing
            None => {
                for (version, origin) in &world.versions {
                    let checksum = new_index_lock.get_checksum(name, version);
                    versions.push(ApworldDiff::Added(version.clone(), checksum));
                }
            }
            // The world was already there before, diff from latest version
            Some(old_world) => {
                for (version, origin) in &world.versions {
                    if old_world.versions.contains_key(version) {
                        continue;
                    }
                    let checksum = new_index_lock.get_checksum(name, version);
                    versions.push(ApworldDiff::Added(version.clone(), checksum));
                }
            }
        }
    }

    for (name, world) in &old_worlds {
        let mut versions = index_diff.0.entry(name.to_string()).or_default();
        if !new_worlds.contains_key(name.as_str()) {
            for version in old_worlds[name].versions.keys() {
                versions.push(ApworldDiff::Removed(version.clone()));
            }
        }
    }

    Ok(index_diff)
}

fn apworld_version_from_precise(precise: &Option<String>) -> Result<Option<(String, Version)>> {
    if let Some(precise) = precise {
        let parts = precise.splitn(2, ':').collect::<Vec<_>>();
        if parts.len() != 2 {
            anyhow::bail!("Precise version need to be of the form <apworld>:<version>");
        }

        Ok(Some((parts[0].to_string(), parts[1].parse::<Version>()?)))
    } else {
        Ok(None)
    }
}

async fn install(
    index_path: &Path,
    apworlds_path: &Path,
    destination: &Path,
    precise: &Option<String>,
) -> Result<()> {
    let index_toml = index_path.join("index.toml");
    let index = apwm::Index::new(&index_toml)?;

    std::fs::create_dir_all(destination).context("While creating destination dir")?;
    let target = apworld_version_from_precise(precise)?;

    for (world_name, world) in &index.worlds {
        let version = if let Some((ref target_apworld, ref target_version)) = target {
            if target_apworld != world_name {
                continue;
            }
            target_version
        } else {
            let Some((version, _)) = world.get_latest_release() else {
                continue;
            };
            version
        };

        let apworld_path = index.get_world_local_path(apworlds_path, world_name, version);
        let destination = destination.join(format!("{}.apworld", world_name));
        std::fs::copy(&apworld_path, &destination)
            .with_context(|| format!("Cannot copy {:?} to {:?}", &apworld_path, &destination))?;
    }

    Ok(())
}
