//! Per-user workspace preferences. These never enter project data or music history.
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::{Path, PathBuf}};

const MAX_SETTINGS_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    pub version: u32,
    pub inspector_visible: bool,
    pub inspector_pinned: bool,
    pub inspector_width: f32,
    /// Negative means the arrangement height is adaptive.
    pub workspace_split: f32,
    pub workspace_focus: i32,
    pub velocity_visible: bool,
    pub arrangement_zoom: f32,
    pub density: i32,
}
impl Default for UiPreferences {
    fn default() -> Self {
        Self { version: 1, inspector_visible: false, inspector_pinned: true, inspector_width: 240.0, workspace_split: -1.0, workspace_focus: 0,
            velocity_visible: false, arrangement_zoom: 48.0, density: 0 }
    }
}
impl UiPreferences {
    fn sanitize(mut self) -> Self {
        if !self.inspector_width.is_finite() { self.inspector_width = 240.0; }
        self.inspector_width = self.inspector_width.clamp(220.0, 400.0);
        if !self.workspace_split.is_finite() { self.workspace_split = -1.0; }
        if self.workspace_split < 0.0 { self.workspace_split = -1.0; }
        else { self.workspace_split = self.workspace_split.clamp(0.0, 1.0); }
        if !(0..=2).contains(&self.workspace_focus) { self.workspace_focus = 0; }
        if !self.arrangement_zoom.is_finite() { self.arrangement_zoom = 48.0; }
        self.arrangement_zoom = self.arrangement_zoom.clamp(12.0, 192.0);
        if !(0..=2).contains(&self.density) { self.density = 0; }
        self
    }
    pub fn path() -> Result<PathBuf, String> {
        Ok(crate::project_library::settings_path()?.with_file_name("ui.json"))
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Cannot read workspace preferences: {e}")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_SETTINGS_BYTES { return Err("Workspace preferences are too large".into()); }
        let value: Self = serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read workspace preferences: {e}"))?;
        if value.version != 1 { return Err(format!("Unsupported workspace preferences version {}", value.version)); }
        Ok(value.sanitize())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if self.version != 1 { return Err(format!("Unsupported workspace preferences version {}", self.version)); }
        let parent = path.parent().ok_or("Invalid workspace preferences path")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.clone().sanitize()).map_err(|e| e.to_string())?;
        crate::storage::atomic_write(path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_are_separate_and_clamped() {
        let dir = std::env::temp_dir().join(format!("opendaw-ui-preferences-{}", std::process::id()));
        let path = dir.join("ui.json");
        let value = UiPreferences { version: 1, inspector_visible: true, inspector_pinned: true, inspector_width: 275.0, workspace_split: 0.42, workspace_focus: 2,
            velocity_visible: true, arrangement_zoom: 96.0, density: 1 };
        value.save(&path).unwrap();
        assert_eq!(UiPreferences::load(&path).unwrap(), value);
        fs::write(&path, br#"{"workspace_split":99,"workspace_focus":20,"arrangement_zoom":900,"density":9}"#).unwrap();
        let loaded = UiPreferences::load(&path).unwrap();
        assert_eq!(loaded.workspace_split, 1.0);
        assert_eq!(loaded.workspace_focus, 0);
        assert_eq!(loaded.arrangement_zoom, 192.0);
        assert_eq!(loaded.density, 0);
        fs::write(&path, br#"{"version":99}"#).unwrap();
        assert!(UiPreferences::load(&path).is_err());
        assert_eq!(UiPreferences::load(&dir.join("missing.json")).unwrap(), UiPreferences::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
