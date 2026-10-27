//! Per-user project locations. Project contents remain in their own portable folders.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const RECENT_LIMIT: usize = 12;
const SETTINGS_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    recent: Vec<RecentProject>,
    last_folder: Option<PathBuf>,
}

#[derive(Default)]
pub struct ProjectLibrary {
    settings: Settings,
    file: Option<PathBuf>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

pub fn settings_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let base = home().map(|p| p.join("Library/Application Support"));
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|p| p.join(".config")));
    Ok(base
        .ok_or("Cannot find the user settings folder")?
        .join("OpenDAW/projects.json"))
}

impl ProjectLibrary {
    /// Per-project UI preferences share the settings root, not the project folder.
    /// An in-memory library intentionally has no persistent side effects.
    pub fn view_state_dir(&self) -> Option<PathBuf> {
        self.file.as_ref().and_then(|file|file.parent()).map(|base|base.join("project-views"))
    }
    pub fn load(file: PathBuf) -> Result<Self, String> {
        let settings = match fs::File::open(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(format!("Cannot read recent projects: {e}")),
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(SETTINGS_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() as u64 > SETTINGS_LIMIT {
                    return Err("Recent-project settings are too large".into());
                }
                serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Cannot read recent projects: {e}"))?
            }
        };
        let mut library = Self {
            settings,
            file: Some(file),
        };
        library.settings.recent.retain(|p| p.path.is_absolute());
        library.settings.recent.truncate(RECENT_LIMIT);
        Ok(library)
    }

    pub fn recent(&self) -> &[RecentProject] {
        &self.settings.recent
    }

    pub fn default_folder(&self) -> PathBuf {
        self.settings.last_folder.clone().unwrap_or_else(|| {
            home()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                .join("Documents/OpenDAW Projects")
        })
    }

    /// Called only after the project was successfully opened, created, or saved.
    pub fn remember(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        self.settings.recent.retain(|p| p.path != path);
        self.settings.last_folder = path.parent().map(Path::to_path_buf);
        self.settings.recent.insert(
            0,
            RecentProject {
                name: name.into(),
                path,
            },
        );
        self.settings.recent.truncate(RECENT_LIMIT);
        if let Some(file) = &self.file {
            fs::create_dir_all(file.parent().ok_or("Invalid settings path")?)
                .map_err(|e| e.to_string())?;
            crate::storage::atomic_write(
                file,
                &serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?,
            )?;
        }
        Ok(())
    }
}

/// New projects always get a fresh child directory; never replace an existing song.
pub fn new_project_path(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 100
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return Err("Choose a project name of 1–100 bytes without path separators or special filename characters".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("This project name is reserved by the operating system".into());
    }
    if parent.as_os_str().is_empty() {
        return Err("Choose a save folder".into());
    }
    Ok(parent.join(format!("{name}.opendaw")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_projects_survive_restart_deduplicate_and_remain_bounded() {
        let root = std::env::temp_dir().join(format!("opendaw-recents-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("settings/projects.json");
        let mut library = ProjectLibrary::load(file.clone()).unwrap();
        for i in 0..15 {
            let path = root.join(format!("Song {i}.opendaw"));
            fs::create_dir_all(&path).unwrap();
            library.remember(&path, &format!("Song {i}")).unwrap();
        }
        library
            .remember(&root.join("Song 7.opendaw"), "Renamed")
            .unwrap();
        let restored = ProjectLibrary::load(file).unwrap();
        assert_eq!(restored.recent().len(), RECENT_LIMIT);
        assert_eq!(restored.recent()[0].name, "Renamed");
        assert_eq!(
            restored
                .recent()
                .iter()
                .filter(|p| p.path.ends_with("Song 7.opendaw"))
                .count(),
            1
        );
        assert_eq!(restored.default_folder(), root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn project_names_cannot_escape_the_selected_folder() {
        for name in [
            "",
            " ",
            ".",
            "..",
            "../other",
            "/tmp/song",
            "a\\b",
            "bad:name",
            "NUL",
            "COM1",
            "end.",
        ] {
            assert!(
                new_project_path(Path::new("/songs"), name).is_err(),
                "{name}"
            );
        }
        assert_eq!(
            new_project_path(Path::new("/songs"), " My song ").unwrap(),
            Path::new("/songs/My song.opendaw")
        );
    }
}
