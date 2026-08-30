use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, PartialEq)]
pub struct Settings {
    pub player: Option<String>,
    pub import_paths: Vec<PathBuf>,
    pub keep_recent: usize,
}

#[derive(Default, Deserialize)]
struct RawSettings {
    #[serde(default)]
    player: RawPlayer,
    #[serde(default, rename = "import")]
    import_settings: RawImport,
    #[serde(default)]
    history: RawHistory,
}

#[derive(Default, Deserialize)]
struct RawPlayer {
    steam_id: Option<String>,
    name: Option<String>,
}

#[derive(Default, Deserialize)]
struct RawImport {
    paths: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
struct RawHistory {
    keep_recent: Option<usize>,
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg_home(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(fallback))
}

pub fn config_home() -> PathBuf {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

pub fn state_home() -> PathBuf {
    xdg_home("XDG_STATE_HOME", ".local/state")
}

pub fn data_home() -> PathBuf {
    xdg_home("XDG_DATA_HOME", ".local/share")
}

pub fn cache_home() -> PathBuf {
    xdg_home("XDG_CACHE_HOME", ".cache")
}

pub fn default_import_paths() -> Vec<PathBuf> {
    vec![
        data_home().join("omarcs/demos"),
        home_dir().join("Downloads"),
        data_home().join("Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo"),
    ]
}

pub fn load_settings(path: Option<&Path>) -> Result<Settings> {
    let owned_path = config_home().join("omarcs/config.toml");
    let path = path.unwrap_or(&owned_path);
    let raw = if path.exists() {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str::<RawSettings>(&text)
            .with_context(|| format!("parsing {}", path.display()))?
    } else {
        RawSettings::default()
    };
    let import_paths = raw
        .import_settings
        .paths
        .map_or_else(default_import_paths, |paths| {
            paths.into_iter().map(|path| expand_path(&path)).collect()
        });
    Ok(Settings {
        player: raw.player.steam_id.or(raw.player.name),
        import_paths,
        keep_recent: raw.history.keep_recent.unwrap_or(20).clamp(1, 100),
    })
}

fn expand_path(path: &str) -> PathBuf {
    let expanded = if path == "~" {
        home_dir().to_string_lossy().into_owned()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest).to_string_lossy().into_owned()
    } else {
        path.to_owned()
    };
    let variable =
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    PathBuf::from(
        variable
            .replace_all(&expanded, |captures: &regex::Captures<'_>| {
                let name = captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .unwrap()
                    .as_str();
                env::var(name).unwrap_or_else(|_| captures[0].to_owned())
            })
            .into_owned(),
    )
}

pub fn steam_loginusers_paths() -> Vec<PathBuf> {
    vec![
        data_home().join("Steam/config/loginusers.vdf"),
        home_dir().join(".steam/steam/config/loginusers.vdf"),
    ]
}

pub fn detect_active_steam_id(paths: Option<&[PathBuf]>) -> Option<String> {
    let defaults = steam_loginusers_paths();
    let block = Regex::new(r#"(?s)"(7656\d{13})"\s*\{(.*?)\n\s*\}"#).unwrap();
    let recent = Regex::new(r#"(?i)"MostRecent"\s+"1""#).unwrap();
    for path in paths.unwrap_or(&defaults) {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let mut fallback = None;
        for captures in block.captures_iter(&text) {
            let steam_id = captures[1].to_owned();
            fallback.get_or_insert_with(|| steam_id.clone());
            if recent.is_match(&captures[2]) {
                return Some(steam_id);
            }
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_most_recent_steam_account() {
        let root = std::env::temp_dir().join(format!("omarcs-config-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("loginusers.vdf");
        fs::write(
            &path,
            r#""users"
{
  "76561198000000001" { "MostRecent" "0"
  }
  "76561198000000002" { "MostRecent" "1"
  }
}"#,
        )
        .unwrap();
        assert_eq!(
            detect_active_steam_id(Some(&[path])),
            Some("76561198000000002".to_owned())
        );
        fs::remove_dir_all(root).unwrap();
    }
}
