//! Native file pickers and message boxes.

use crate::ffi::*;
use crate::window::Window;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

const PATH_CAPACITY: usize = 4096;

/// A filter string such as "Images\0*.png\0" with its required double terminator.
fn filter_buffer(filter: &str) -> Vec<u16> {
    filter.encode_utf16().chain([0, 0]).collect()
}

fn run_dialog(
    window: Window,
    filter: &str,
    start: Option<&Path>,
    default_ext: Option<&str>,
    flags: u32,
    save: bool,
    title: Option<&str>,
) -> Option<PathBuf> {
    if window.is_detached() {
        return None;
    }
    let title = title.map(wide);
    let filter = filter_buffer(filter);
    let ext = default_ext.map(wide);
    let mut file = vec![0u16; PATH_CAPACITY];
    // The folder goes separately: a suggested name with a full path can open somewhere else.
    let folder: Option<Vec<u16>> = start
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.as_os_str().encode_wide().chain(Some(0)).collect());
    if let Some(name) = start.and_then(Path::file_name) {
        let existing: Vec<u16> = name.encode_wide().take(PATH_CAPACITY - 1).collect();
        file[..existing.len()].copy_from_slice(&existing);
    }
    let mut dialog = OPENFILENAMEW {
        size: std::mem::size_of::<OPENFILENAMEW>() as u32,
        owner: window.raw(),
        instance: null_mut(),
        filter: filter.as_ptr(),
        custom_filter: null_mut(),
        max_custom: 0,
        filter_index: 1,
        file: file.as_mut_ptr(),
        max_file: PATH_CAPACITY as u32,
        file_title: null_mut(),
        max_file_title: 0,
        initial_dir: folder.as_ref().map_or(null(), |f| f.as_ptr()),
        title: title.as_ref().map_or(null(), |t| t.as_ptr()),
        flags: flags | OFN_NOCHANGEDIR,
        file_offset: 0,
        extension_offset: 0,
        default_ext: ext.as_ref().map_or(null(), |e| e.as_ptr()),
        cust_data: 0,
        hook: null(),
        template: null(),
        reserved_ptr: null_mut(),
        reserved: 0,
        flags_ex: 0,
    };
    // SAFETY: `dialog` and every buffer it points to outlive this blocking call, and
    // `max_file` matches the length of `file`.
    let accepted = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        }
    };
    if accepted == 0 {
        return None;
    }
    let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(OsString::from_wide(&file[..end])))
}

/// Shows an open dialog. `filter` uses NUL-separated description and pattern pairs.
pub fn open_file_dialog(window: Window, filter: &str) -> Option<PathBuf> {
    run_dialog(
        window,
        filter,
        None,
        None,
        OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        false,
        None,
    )
}

/// An open dialog with its own title, such as one that says which plugin asks.
pub fn open_file_dialog_titled(window: Window, filter: &str, title: &str) -> Option<PathBuf> {
    run_dialog(
        window,
        filter,
        None,
        None,
        OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        false,
        Some(title),
    )
}

pub fn save_file_dialog(
    window: Window,
    filter: &str,
    extension: &str,
    start: Option<&Path>,
) -> Option<PathBuf> {
    run_dialog(
        window,
        filter,
        start,
        Some(extension),
        OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        true,
        None,
    )
}

/// A save dialog with its own title, such as one that says which plugin asks. It starts
/// with no folder or name of the asker's, confirms overwriting, refuses read-only files, and
/// takes a shortcut as itself rather than the file it points to.
pub fn save_file_dialog_titled(
    window: Window,
    filter: &str,
    title: &str,
    extension: Option<&str>,
) -> Option<PathBuf> {
    let flags =
        OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOREADONLYRETURN | OFN_NODEREFERENCELINKS;
    run_dialog(window, filter, None, extension, flags, true, Some(title))
}

/// Shows a message box and returns the button pressed. Detached windows answer `None`.
fn message_box(window: Window, message: &str, kind: u32) -> Option<i32> {
    if window.is_detached() {
        return None;
    }
    let (text, caption) = (wide(message), wide("Pikslar"));
    // Ctrl+S under the box beeps rather than going unanswered.
    let _watch = crate::box_keys::BoxKeys::watch();
    // SAFETY: both strings are NUL-terminated and live for this blocking call.
    Some(unsafe { MessageBoxW(window.raw(), text.as_ptr(), caption.as_ptr(), kind) })
}

pub fn show_error(window: Window, message: &str) {
    message_box(window, message, MB_OK | MB_ICONERROR);
}

/// Confirms replacing a file the normal dialog prompt could not have warned about.
pub fn ask_replace_file(window: Window, name: &str) -> bool {
    let message = format!("{name} already exists. Replace it?");
    message_box(window, &message, MB_YESNO | MB_ICONWARNING) == Some(IDYES)
}

/// Offers work left behind by a session that ended unexpectedly. `None` means nobody was
/// able to answer, which must never be taken as permission to delete the work.
pub fn ask_restore_work(window: Window, name: &str) -> Option<bool> {
    let message = format!(
        "Pikslar closed unexpectedly and kept unsaved work ({name}).\n\n\
         Restore it? Choosing No deletes this copy."
    );
    match message_box(window, &message, MB_YESNO | MB_ICONWARNING) {
        Some(IDYES) => Some(true),
        Some(IDNO) => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeChoice {
    Upgrade,
    SaveCopy,
    Cancel,
}

/// Asks before saving over a project an older version made. With `can_upgrade` false the
/// question offers only a copy, answered Yes, or nothing.
pub fn ask_upgrade(window: Window, question: &str, can_upgrade: bool) -> UpgradeChoice {
    let kind = if can_upgrade {
        MB_YESNOCANCEL
    } else {
        MB_YESNO
    } | MB_ICONWARNING;
    match (message_box(window, question, kind), can_upgrade) {
        (Some(IDYES), true) => UpgradeChoice::Upgrade,
        (Some(IDNO), true) | (Some(IDYES), false) => UpgradeChoice::SaveCopy,
        _ => UpgradeChoice::Cancel,
    }
}

/// What to do about a file another program changed since it was opened or saved here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangedChoice {
    Overwrite,
    SaveAs,
    Cancel,
}

/// Asks whether to save over `name`, which changed on disk. Cancel is the default, so a stray
/// Enter loses nothing. A detached window answers Cancel.
pub fn ask_changed_on_disk(window: Window, name: &str) -> ChangedChoice {
    let message = format!(
        "{name} was changed by another program since it was opened or saved here.

         Yes: save over it with this version.
No: save this version under another name.
         Cancel: save nothing."
    );
    let kind = MB_YESNOCANCEL | MB_ICONWARNING | MB_DEFBUTTON3;
    match message_box(window, &message, kind) {
        Some(IDYES) => ChangedChoice::Overwrite,
        Some(IDNO) => ChangedChoice::SaveAs,
        _ => ChangedChoice::Cancel,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

pub fn ask_save_changes(window: Window) -> SaveChoice {
    // Cancel is the default, so a stray Enter or a key typed into the question loses nothing.
    let kind = MB_YESNOCANCEL | MB_ICONWARNING | MB_DEFBUTTON3;
    match message_box(window, "Save changes to this sprite?", kind) {
        Some(IDYES) => SaveChoice::Save,
        Some(IDNO) => SaveChoice::Discard,
        _ => SaveChoice::Cancel,
    }
}

/// A save would replace a backup an interrupted save left, which may be the only good copy of
/// earlier work. `Some(true)` lets this save replace it, `Some(false)` keeps it and the save does
/// not happen; `None` means nobody could answer, which is taken as keeping it. Nothing here
/// closes or replaces the work being saved.
pub fn ask_replace_backup(window: Window, name: &str) -> Option<bool> {
    let message = format!(
        "A backup from an interrupted save is in the way: {name}.\n\n\
         It may hold earlier work the project itself lost.\n\n\
         Yes: replace it with this save.\n\
         No: keep it, and do not save now. Your work here stays open: use Save as to keep it \
         under another name."
    );
    match message_box(window, &message, MB_YESNO | MB_ICONWARNING) {
        Some(IDYES) => Some(true),
        Some(IDNO) => Some(false),
        _ => None,
    }
}

/// Offers a backup an interrupted save left beside a project. `Some(true)` opens it, `Some(false)`
/// ignores it; `None` means nobody was able to answer, which must never be taken as leave to
/// replace it.
pub fn ask_open_backup(window: Window, name: &str) -> Option<bool> {
    let message = format!(
        "A backup from an interrupted save was found: {name}.\n\n\
         It may hold work the project itself lost.\n\n\
         Yes: open the backup's work as a new document (the files stay as they are).\n\
         No: ignore it; the next save of this project replaces it."
    );
    match message_box(window, &message, MB_YESNO | MB_ICONWARNING) {
        Some(IDYES) => Some(true),
        Some(IDNO) => Some(false),
        _ => None,
    }
}

/// Before every file's kept undo history is deleted. Only Yes deletes it; nobody answering
/// keeps it.
pub fn ask_forget_all_history(window: Window) -> bool {
    let message = "Forget the kept undo history of every file?\n\n\
                   The files themselves stay as they are. This cannot be undone.";
    message_box(window, message, MB_YESNO | MB_ICONWARNING) == Some(IDYES)
}
