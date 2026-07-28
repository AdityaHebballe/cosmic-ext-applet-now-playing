use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub album_color_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            album_color_enabled: true,
        }
    }
}

#[must_use]
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| {
        path.join("cosmic-ext-applet-now-playing")
            .join("album-color-enabled")
    })
}

pub fn load() -> AppSettings {
    let Some(path) = settings_path() else {
        eprintln!("unable to load settings: user config directory not found");
        return AppSettings::default();
    };

    match load_from(&path) {
        Ok(settings) => settings,
        Err(error) if error.kind() == io::ErrorKind::NotFound => AppSettings::default(),
        Err(error) => {
            eprintln!("unable to load settings from {}: {error}", path.display());
            AppSettings::default()
        }
    }
}

pub fn save(settings: AppSettings) -> io::Result<()> {
    let path = settings_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "user config directory not found")
    })?;
    save_to(&path, settings)
}

fn load_from(path: &Path) -> io::Result<AppSettings> {
    let value = fs::read_to_string(path)?;
    let album_color_enabled = match value.trim() {
        "true" => true,
        "false" => false,
        invalid => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid album color setting: {invalid}"),
            ));
        }
    };

    Ok(AppSettings {
        album_color_enabled,
    })
}

fn save_to(path: &Path, settings: AppSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        path,
        if settings.album_color_enabled {
            "true\n"
        } else {
            "false\n"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_settings() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/setting");
        let expected = AppSettings {
            album_color_enabled: false,
        };

        save_to(&path, expected).unwrap();

        assert_eq!(load_from(&path).unwrap(), expected);
    }

    #[test]
    fn rejects_invalid_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("setting");
        fs::write(&path, "sometimes\n").unwrap();

        assert_eq!(
            load_from(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn reports_write_failures() {
        let directory = tempdir().unwrap();

        assert!(save_to(directory.path(), AppSettings::default()).is_err());
    }
}
