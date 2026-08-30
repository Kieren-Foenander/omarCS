use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config;
use crate::report;
use crate::storage::Store;

pub fn demo_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = HashSet::new();
    for path in paths {
        collect_demos(path, &mut found);
    }
    let mut found = found.into_iter().collect::<Vec<_>>();
    found.sort_by_key(|path| {
        path.metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    found
}

fn collect_demos(path: &Path, found: &mut HashSet<PathBuf>) {
    if path.is_file() {
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dem"))
        {
            found.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() || kind.is_file() {
            collect_demos(&entry.path(), found);
        }
    }
}

pub fn import_paths(paths: &[PathBuf], player: Option<&str>, quiet: bool) -> Result<u8> {
    let settings = config::load_settings(None)?;
    let detected = config::detect_active_steam_id(None);
    let selector = player.map(str::to_owned).or(settings.player).or(detected);
    let store = Store::open(None)?;
    let files = demo_files(paths);
    if files.is_empty() {
        store.publish(settings.keep_recent)?;
        if !quiet {
            println!("No .dem files found.");
        }
        return Ok(0);
    }
    let Some(selector) = selector else {
        let message =
            "Could not detect your Steam account. Pass --player STEAMID64 or player name.";
        store.write_status("error", message)?;
        eprintln!("{message}");
        return Ok(2);
    };

    store.write_status(
        "analyzing",
        &format!(
            "Checking {} demo{}…",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ),
    )?;
    let mut imported = 0;
    let mut failures = Vec::new();
    for path in files {
        let outcome = (|| -> Result<()> {
            let checksum = report::checksum_path(&path)?;
            if store.has_checksum(&checksum, report::ANALYSIS_VERSION)? {
                return Ok(());
            }
            if !quiet {
                println!(
                    "Analyzing {}…",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            let report = report::generate(&path, &selector)?;
            if report.checksum != checksum {
                anyhow::bail!("Match Report checksum did not match the Demo digest");
            }
            store.save_match(&serde_json::to_value(report)?)?;
            imported += 1;
            Ok(())
        })();
        if let Err(error) = outcome {
            failures.push(format!(
                "{}: {error:#}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    if !failures.is_empty() && imported == 0 && store.matches(1)?.is_empty() {
        store.write_status("error", &failures[0])?;
    } else {
        store.publish(settings.keep_recent)?;
    }
    if !quiet {
        println!(
            "Imported {imported} new match{}.",
            if imported == 1 { "" } else { "es" }
        );
        for failure in &failures {
            eprintln!("Warning: {failure}");
        }
    }
    Ok(u8::from(!failures.is_empty() && imported == 0))
}

pub fn refresh(player: Option<&str>, quiet: bool) -> Result<u8> {
    let settings = config::load_settings(None).context("loading settings")?;
    import_paths(&settings.import_paths, player, quiet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn finds_demos_recursively_and_orders_by_mtime() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("omarcs-scan-{}-{stamp}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("ignore.txt"), b"x").unwrap();
        fs::write(root.join("nested/match.dem"), b"x").unwrap();
        let found = demo_files(std::slice::from_ref(&root));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "match.dem");
        fs::remove_dir_all(root).unwrap();
    }
}
