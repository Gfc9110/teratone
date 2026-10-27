//! Project commands and native folder dialogs on the UI event loop.
use crate::AppWindow;
use opendaw_app::{
    controller::{ProjectTarget, Request, UnsavedChanges},
    mailbox::Mailbox,
    project_library,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

fn queue(ui: &AppWindow, mailbox: &RefCell<Mailbox>, request: Request) {
    match mailbox.borrow_mut().enqueue(request) {
        Ok(()) => ui.set_action_error("".into()),
        Err(error) => ui.set_action_error(error.into()),
    }
}

fn choose_folder(
    ui: &AppWindow,
    initial: &str,
    title: &str,
    apply: impl FnOnce(&AppWindow, PathBuf) + 'static,
) {
    if ui.get_folder_dialog_busy() { return; }
    ui.set_folder_dialog_busy(true);
    ui.invoke_cancel_interactions();
    let mut directory = PathBuf::from(initial);
    while !directory.is_dir() && directory.pop() {}
    let dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_directory(directory)
        .set_can_create_directories(true);
    let weak = ui.as_weak();
    if let Err(error) = slint::spawn_local(async move {
        let folder = dialog.pick_folder().await;
        if let Some(ui) = weak.upgrade() {
            ui.set_folder_dialog_busy(false);
            if let Some(folder) = folder {
                apply(&ui, folder.path().to_path_buf());
            }
        }
    }) {
        ui.set_folder_dialog_busy(false);
        ui.set_action_error(format!("Cannot open folder chooser: {error}").into());
    }
}

pub fn reveal_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("The project folder is no longer available".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(&path).spawn();
    #[cfg(windows)]
    let result = Command::new("explorer.exe").arg(&path).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let result = Command::new("xdg-open").arg(&path).spawn();
    result
        .map(|mut child| {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
        .map_err(|e| format!("Cannot open the project folder: {e}"))
}

pub fn bind(ui: &AppWindow, mailbox: Rc<RefCell<Mailbox>>) {
    let weak = ui.as_weak();
    let m = mailbox.clone();
    ui.on_save_project(move |path| {
        if let Some(ui) = weak.upgrade() {
            queue(&ui, &m, Request::Save(PathBuf::from(path.as_str())));
        }
    });
    let weak = ui.as_weak();
    let m = mailbox.clone();
    ui.on_open_project(move |path| {
        if let Some(ui) = weak.upgrade() {
            queue(&ui, &m, Request::Open(PathBuf::from(path.as_str())));
        }
    });
    let weak = ui.as_weak();
    let m = mailbox.clone();
    ui.on_switch_project(move |kind, path, name, policy| {
        if let Some(ui) = weak.upgrade() {
            let target = if kind == 1 {
                ProjectTarget::New {
                    name: name.into(),
                    parent: PathBuf::from(path.as_str()),
                }
            } else {
                ProjectTarget::Existing(PathBuf::from(path.as_str()))
            };
            let unsaved = match policy {
                1 => UnsavedChanges::Save,
                2 => UnsavedChanges::Discard,
                _ => UnsavedChanges::Reject,
            };
            queue(&ui, &m, Request::SwitchProject { target, unsaved });
        }
    });
    let weak = ui.as_weak();
    let m = mailbox;
    ui.on_save_project_as(move |parent, name| {
        if let Some(ui) = weak.upgrade() {
            match project_library::new_project_path(Path::new(parent.as_str()), name.as_str()) {
                Ok(path) => queue(&ui, &m, Request::SaveAs(path)),
                Err(error) => ui.set_action_error(error.into()),
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_browse_folder(move |target| {
        if let Some(ui) = weak.upgrade() {
            let initial = if target == 1 {
                ui.get_save_as_parent()
            } else {
                ui.get_new_parent()
            };
            choose_folder(
                &ui,
                initial.as_str(),
                "Choose where to save OpenDAW projects",
                move |ui, path| {
                    if target == 1 {
                        ui.set_save_as_parent(path.to_string_lossy().into_owned().into());
                    } else {
                        ui.set_new_parent(path.to_string_lossy().into_owned().into());
                    }
                },
            );
        }
    });
    let weak = ui.as_weak();
    ui.on_choose_open_project(move || {
        if let Some(ui) = weak.upgrade() {
            choose_folder(
                &ui,
                ui.get_new_parent().as_str(),
                "Open an .opendaw folder containing project.json",
                |ui, path| {
                    ui.invoke_request_switch(
                        0,
                        path.to_string_lossy().into_owned().into(),
                        "".into(),
                    );
                },
            );
        }
    });
    let weak = ui.as_weak();
    ui.on_reveal_project(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = reveal_folder(Path::new(ui.get_project_path().as_str())) {
                ui.set_action_error(error.into());
            }
        }
    });
}
