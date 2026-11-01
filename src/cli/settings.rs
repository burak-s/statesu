use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::error::CliError;

/// Local CLI state: server address, saved token and selected blog. The file
/// lives in the user config directory and is written user-only, because it
/// holds the personal access token secret.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blog_id: Option<String>,
}

impl Settings {
    pub fn load() -> Result<Self, CliError> {
        Self::load_from(config_base()?)
    }

    pub fn load_from(base: PathBuf) -> Result<Self, CliError> {
        let path = Self::path_in(base);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|err| cannot_read(path.display(), err))?;
        serde_json::from_str(&text).map_err(|err| {
            CliError::Config(format!(
                "{} is unreadable ({err}); remove it and log in again",
                path.display()
            ))
        })
    }

    pub fn save(&self) -> Result<(), CliError> {
        self.save_in(config_base()?)
    }

    pub fn save_in(&self, base: PathBuf) -> Result<(), CliError> {
        let path = Self::path_in(base);
        let parent = path
            .parent()
            .ok_or_else(|| CliError::Config("settings path has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|err| cannot_read(parent.display(), err))?;
        let body = serde_json::to_string_pretty(self)
            .map_err(|err| CliError::Config(format!("cannot serialize settings: {err}")))?;
        let mut file = user_only_file(&path)?;
        file.write_all(body.as_bytes())
            .map_err(|err| cannot_read(path.display(), err))?;
        file.sync_all().map_err(CliError::Io)?;
        Ok(())
    }

    pub fn path() -> Result<PathBuf, CliError> {
        Ok(Self::path_in(config_base()?))
    }

    fn path_in(base: PathBuf) -> PathBuf {
        base.join("statesu").join("cli.json")
    }
}

fn cannot_read(path: impl std::fmt::Display, err: std::io::Error) -> CliError {
    CliError::Config(format!("cannot use {path}: {err}"))
}

/// `$XDG_CONFIG_HOME`, else `$HOME/.config` (as XDG recommends), else
/// `$APPDATA` on Windows.
fn config_base() -> Result<PathBuf, CliError> {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => match env::var_os("HOME") {
            Some(home) => Ok(PathBuf::from(home).join(".config")),
            _ => match env::var_os("APPDATA") {
                Some(appdata) => Ok(PathBuf::from(appdata)),
                _ => Err(CliError::Config(
                    "cannot determine a configuration directory".into(),
                )),
            },
        },
    }
}

#[cfg(unix)]
fn user_only_file(path: &Path) -> Result<fs::File, CliError> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|err| cannot_read(path.display(), err))
}

#[cfg(not(unix))]
fn user_only_file(path: &Path) -> Result<fs::File, CliError> {
    fs::File::create(path).map_err(|err| cannot_read(path.display(), err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().to_path_buf();
        let settings = Settings {
            server: Some("https://cms.example.com".into()),
            pat: Some("su_pat_secret".into()),
            pat_id: Some("00000000-0000-0000-0000-000000000000".into()),
            blog_id: Some("00000000-0000-0000-0000-000000000001".into()),
        };

        settings.save_in(base.clone()).expect("save");
        let loaded = Settings::load_from(base.clone()).expect("load");
        assert_eq!(loaded.server, settings.server);
        assert_eq!(loaded.pat, settings.pat);
        assert_eq!(loaded.blog_id, settings.blog_id);

        // Unix mode: the token file stays user-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(Settings::path_in(base.clone()))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Corrupt settings never panic; they report a recoverable error.
        std::fs::write(Settings::path_in(base), "{").expect("write");
        assert!(Settings::load_from(dir.path().to_path_buf()).is_err());
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = Settings::load_from(dir.path().to_path_buf()).expect("defaults");
        assert_eq!(settings.server, None);
        assert_eq!(settings.pat, None);
        assert_eq!(settings.blog_id, None);
    }
}
