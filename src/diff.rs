use std::{collections::BTreeMap, path::Path, process::Command, str::FromStr};
    let out = Command::new("git")
        .arg("diff")
        .arg("--no-index")
        .arg(from)
        .arg(to)
        .output()?;

    Ok(String::from_utf8(out.stdout)?
        .replace(from.to_str().unwrap(), "")
        .replace(to.to_str().unwrap(), ""))
            archive.write_fmt(format_args!("{}\n", version))?;
            apworld_name: "foobar".to_string(),
            world_name: "New World".to_string(),
                        "diff --git a/VERSION b/VERSION\nnew file mode 100644\nindex 0000000..8acdd82\n--- /dev/null\n+++ b/VERSION\n@@ -0,0 +1 @@\n+0.0.1\n".to_string()
                        "diff --git a/VERSION b/VERSION\nindex 8acdd82..4e379d2 100644\n--- a/VERSION\n+++ b/VERSION\n@@ -1 +1 @@\n-0.0.1\n+0.0.2\n".to_string()
                        "diff --git a/VERSION b/VERSION\nindex 4e379d2..bcab45a 100644\n--- a/VERSION\n+++ b/VERSION\n@@ -1 +1 @@\n-0.0.2\n+0.0.3\n".to_string()
            apworld_name: "foobar".to_string(),
            world_name: "Old World".to_string(),
            apworld_name: "foobar".to_string(),
            world_name: "World".to_string(),
                        "diff --git a/VERSION b/VERSION\nindex bcab45a..81340c7 100644\n--- a/VERSION\n+++ b/VERSION\n@@ -1 +1 @@\n-0.0.3\n+0.0.4\n".to_string()