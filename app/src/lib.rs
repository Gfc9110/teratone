pub mod arrangement;
pub mod bridge;
pub mod editor;
pub mod midi_view;
pub mod note_presentation;
pub mod project_view_state;
pub mod midi_references;
pub mod velocity_lane;
pub mod project;
pub mod storage;
pub mod project_library;
pub mod ui_preferences;
#[cfg(feature = "ui")]
pub mod controller;

#[cfg(feature = "ui")]
pub mod mailbox;

pub mod synth;

pub mod drums;
