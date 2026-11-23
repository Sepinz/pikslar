//! Host functions for files a plugin may read (`PickFileToRead`) or write
//! (`PickFileToWrite`): only one the user chooses in the host's own open or save dialog,
//! through a handle that means nothing outside the run. The path stays with the host
//! ([`pikslar_plugin::files::FileGrants`]); the script gets a small number, the file's text
//! and its name, never where it is.
//!
//! A read is made once per handle, a piece at a time, each piece paid for before it is read,
//! so a cancel stops it between pieces; reading the handle again gives the same text at no
//! further cost. A write is only asked for: the text stays in the run's memory, and is
//! written when the run has succeeded, just before its undo step is kept ([`commit`]). A run
//! that fails, is stopped or is cancelled writes no file. No file goes in or under the
//! folders the app protects ([`FileAccess::with_protected`]).

use crate::binding::Binding;
use crate::cost;
use crate::values::{int, Args};
use pikslar_plugin::files::{
    Access, Disk, FileError, FileGrants, FileHandle, FilePicker, FileRequest, Reading,
};
use pikslar_plugin::CapabilitySet;
use pikslar_script::{HostCx, HostError, Value};
use std::sync::atomic::{AtomicBool, Ordering};

type Out = Result<Value, HostError>;

/// Most file kinds one dialog lists, as `FileRequest` takes them.
const MOST_KINDS: usize = 8;

/// Bytes of a file read or written for one unit of fuel.
const BYTES_PER_UNIT: u64 = 64;

/// Bytes read at a time, each piece paid for first; the cancel flag is read between pieces.
const PIECE: u64 = 1 << 20;

/// The files one run may reach: what it was granted, and the dialog that asks the user.
pub struct FileAccess {
    grants: FileGrants,
    picker: Box<dyn FilePicker + Send>,
}

impl FileAccess {
    /// For one run of the plugin `plugin_name`, which was granted `granted` at install.
    pub fn new(
        plugin_name: &str,
        granted: CapabilitySet,
        picker: Box<dyn FilePicker + Send>,
    ) -> FileAccess {
        FileAccess {
            grants: FileGrants::for_run(plugin_name, granted),
            picker,
        }
    }

    /// The same access, never writing in or under `folders` (the app gives its settings and
    /// plugins folders), compared by where they really are.
    pub fn with_protected(self, folders: Vec<std::path::PathBuf>) -> FileAccess {
        FileAccess {
            grants: self.grants.with_protected(folders),
            ..self
        }
    }

    /// The same access, writing files through `disk` (the app gives the system's replace).
    pub fn with_disk(self, disk: Box<dyn Disk>) -> FileAccess {
        FileAccess {
            grants: self.grants.with_disk(disk),
            ..self
        }
    }
}

/// What the run holds of its files, as script values on the run's own thread: the text of
/// each file read, and the text of each write waiting for the end of the run.
#[derive(Default)]
pub(crate) struct Held {
    read: Vec<(FileHandle, Value)>,
    writes: Vec<(FileHandle, Value)>,
    /// The writes were put in place.
    pub(crate) written: bool,
}

fn access<'b>(b: &'b mut Binding<'_, '_>, name: &str) -> Result<&'b mut FileAccess, HostError> {
    b.files
        .as_mut()
        .ok_or_else(|| HostError::new(format!("{name}: this run has no way to ask for files")))
}

fn handle(a: &Args<'_>) -> Result<FileHandle, HostError> {
    let n = u32::try_from(a.int(0)?).map_err(|_| HostError::new("that is not a file handle"))?;
    Ok(FileHandle::from_number(n))
}

fn failed(name: &str, e: impl std::fmt::Display) -> HostError {
    HostError::new(format!("{name}: {e}"))
}

fn pick(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>, how: Access) -> Out {
    cx.charge(cost::EDIT)?;
    let title = a.text(0)?;
    let kinds = a.texts(1, MOST_KINDS)?;
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let request = FileRequest::new(how, &title, &kinds).map_err(|e| failed(a.name, e))?;
    let files = access(b, a.name)?;
    match files.grants.pick(files.picker.as_mut(), &request) {
        Ok(Some(h)) => Ok(int(h.number())),
        Ok(None) => Ok(Value::Nil),
        Err(e) => Err(failed(a.name, e)),
    }
}

/// `pick_file_to_read(title, kinds)`: a handle, or nil when the user cancels.
pub fn pick_file_to_read(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    pick(b, a, cx, Access::Read)
}

/// `pick_file_to_write(title, kinds)`: a handle, or nil when the user cancels.
pub fn pick_file_to_write(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    pick(b, a, cx, Access::Write)
}

/// `read_file(handle)`: the whole file the user chose, as text.
pub fn read_file(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::READ)?;
    let h = handle(a)?;
    if let Some((_, text)) = b.held.read.iter().find(|(read, _)| *read == h) {
        return Ok(text.clone());
    }
    let grants = &access(b, a.name)?.grants;
    // Refused from the file's length, before it is read or the read paid for, when the
    // script could not hold it as text or pay for it; then read no further than that length.
    let len = grants.len(h).map_err(|e| failed(a.name, e))?;
    cx.need_room(usize::try_from(len).unwrap_or(usize::MAX))?;
    if len / BYTES_PER_UNIT > cx.fuel_left() {
        cx.charge(len / BYTES_PER_UNIT)?;
    }
    let mut reading = grants.start_read(h, len).map_err(|e| failed(a.name, e))?;
    let bytes = read_pieces(&mut reading, &mut |units| cx.charge(units))
        .map_err(|e| e.into_host(a.name))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| HostError::new(format!("{}: the file is not UTF-8 text", a.name)))?;
    let text = cx.new_str(text)?;
    b.held.read.push((h, text.clone()));
    Ok(text)
}

/// Why reading pieces stopped.
#[derive(Debug)]
pub(crate) enum PieceError {
    Paying(HostError),
    File(FileError),
}

impl PieceError {
    fn into_host(self, name: &str) -> HostError {
        match self {
            PieceError::Paying(e) => e,
            PieceError::File(e) => failed(name, e),
        }
    }
}

/// The whole of `reading`, a [`PIECE`] at a time, each paid for with `pay` before it is
/// read: a refusal to pay (no fuel left, or the run cancelled) stops the read there.
pub(crate) fn read_pieces(
    reading: &mut Reading,
    pay: &mut dyn FnMut(u64) -> Result<(), HostError>,
) -> Result<Vec<u8>, PieceError> {
    let len = reading.len();
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    loop {
        let left = len.saturating_sub(bytes.len() as u64);
        pay(left.min(PIECE) / BYTES_PER_UNIT).map_err(PieceError::Paying)?;
        let piece = usize::try_from(PIECE).unwrap_or(usize::MAX);
        if reading
            .read_some(&mut bytes, piece)
            .map_err(PieceError::File)?
        {
            return Ok(bytes);
        }
    }
}

/// `write_file(handle, text)`: asks for the file the user chose to save as to hold `text`,
/// once. It is written when the run has succeeded; nil now.
pub fn write_file(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::EDIT)?;
    let h = handle(a)?;
    let text = a.string(1)?;
    let len = text.as_str().map_or(0, str::len) as u64;
    let files = access(b, a.name)?;
    files
        .grants
        .claim_write(h, len)
        .map_err(|e| failed(a.name, e))?;
    cx.charge(len / BYTES_PER_UNIT)?;
    b.held.writes.push((h, text.clone()));
    Ok(Value::Nil)
}

/// `file_name(handle)`: the chosen file's name without its folder, for a message.
pub fn file_name(b: &mut Binding<'_, '_>, a: &Args<'_>, cx: &mut HostCx<'_, '_>) -> Out {
    cx.charge(cost::READ)?;
    let h = handle(a)?;
    let name = access(b, a.name)?
        .grants
        .file_name(h)
        .map_err(|e| failed(a.name, e))?;
    cx.new_str(&name)
}

/// At the end of a run that succeeded: writes every file it asked to, all or none. A cancel
/// seen before the files are put in place writes none.
pub(crate) fn commit(b: &mut Binding<'_, '_>, cancel: &AtomicBool) -> Result<(), FileError> {
    let writes = std::mem::take(&mut b.held.writes);
    let Some(files) = b.files.as_mut() else {
        return Ok(());
    };
    if writes.is_empty() {
        return Ok(());
    }
    let bytes: Vec<(FileHandle, &[u8])> = writes
        .iter()
        .map(|(h, text)| (*h, text.as_str().map_or(&[][..], str::as_bytes)))
        .collect();
    files
        .grants
        .commit(&bytes, &|| !cancel.load(Ordering::Relaxed))?;
    b.held.written = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_plugin::Capability as Word;

    struct Chosen(std::path::PathBuf);

    impl FilePicker for Chosen {
        fn pick(&mut self, _: &str, _: &FileRequest) -> Option<std::path::PathBuf> {
            Some(self.0.clone())
        }
    }

    #[test]
    fn a_read_stops_at_the_first_piece_it_cannot_pay_for() {
        let dir = std::env::temp_dir().join(format!("pikslar-pieces-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let path = dir.join("big.txt");
        std::fs::write(&path, vec![b'a'; 3 << 20]).expect("written");
        let mut grants = FileGrants::for_run("P", CapabilitySet::NONE.with(Word::PickFileToRead));
        let request = FileRequest::new(Access::Read, "Open", &[]).expect("request");
        let h = grants
            .pick(&mut Chosen(path), &request)
            .expect("picked")
            .expect("chosen");
        let mut reading = grants.start_read(h, 3 << 20).expect("opened");
        let mut paid = 0;
        let mut pay = |units: u64| match paid {
            1 => Err(HostError::new("cancelled")),
            _ => {
                paid += 1;
                assert_eq!(units, PIECE / BYTES_PER_UNIT);
                Ok(())
            }
        };
        assert!(matches!(
            read_pieces(&mut reading, &mut pay),
            Err(PieceError::Paying(_))
        ));
        let one_piece = format!("read: {PIECE}");
        assert!(format!("{reading:?}").contains(&one_piece), "{reading:?}");
        let mut reading = grants.start_read(h, 3 << 20).expect("opened");
        let all = read_pieces(&mut reading, &mut |_| Ok(())).expect("read");
        assert_eq!(all.len(), 3 << 20);
        let _ = std::fs::remove_dir_all(dir);
    }
}
