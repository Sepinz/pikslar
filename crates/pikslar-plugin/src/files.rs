//! Files only through the user's pickers.
//!
//! A plugin never names a path. With [`Capability::PickFileToRead`] or
//! [`Capability::PickFileToWrite`] it may *ask*: the host shows its own open or save dialog
//! (a [`FilePicker`]), the user chooses or cancels, and the plugin gets back an opaque
//! [`FileHandle`], a small number that means "the file the user chose" for the rest of that
//! one run. The path stays in [`FileGrants`], on the host's side; the plugin can read or
//! write through the handle and learn the file's name for a message, nothing more. A handle
//! from another run, or a made-up number, is refused. A read handle cannot write and a write
//! handle cannot read.
//!
//! The plugin cannot choose the folder the dialog opens in, or a file name to save as: the
//! request carries only a title and the kinds of file wanted, which the dialog shows.

use crate::capability::{Capability, CapabilitySet};
pub use crate::kinds::{is_allowed_extension, is_allowed_name, ALLOWED_EXTENSIONS};
use crate::names::check_display;
use crate::save::{self, Seen, Wanted};
pub use crate::save::{Disk, PlainDisk};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The most files one run may ask the user for, so a plugin cannot flood them with dialogs.
pub const MOST_PICKS_PER_RUN: usize = 8;
/// The largest file a plugin may read or write through a handle.
pub const MOST_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MOST_TITLE_CHARS: usize = 64;
const MOST_EXTENSIONS: usize = 8;

/// "The file the user chose", for one run. Opaque: it holds no path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileHandle(u32);

impl FileHandle {
    /// The number the script sees.
    pub fn number(self) -> u32 {
        self.0
    }

    /// A handle from a number a script gave back. Whether it means anything is decided by
    /// the [`FileGrants`] it is used with.
    pub fn from_number(n: u32) -> FileHandle {
        FileHandle(n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    fn capability(self) -> Capability {
        match self {
            Access::Read => Capability::PickFileToRead,
            Access::Write => Capability::PickFileToWrite,
        }
    }
}

/// What a plugin asks the dialog to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRequest {
    pub access: Access,
    /// Shown as the dialog's title, after the plugin's name.
    pub title: String,
    /// File kinds offered, like `png`; empty for any.
    pub extensions: Vec<String>,
}

impl FileRequest {
    /// A checked request: a title that shows as itself and plain lower-case extensions. A
    /// save may only offer the kinds on [`ALLOWED_EXTENSIONS`].
    #[inline(never)]
    pub fn new(access: Access, title: &str, extensions: &[&str]) -> Result<FileRequest, FileError> {
        check_display("dialog title", title, MOST_TITLE_CHARS).map_err(FileError::BadRequest)?;
        let plain = |e: &&str| {
            (1..=8).contains(&e.len())
                && e.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        };
        if extensions.len() > MOST_EXTENSIONS || !extensions.iter().all(plain) {
            return Err(FileError::BadRequest(
                "file kinds are up to 8 plain extensions like png".to_string(),
            ));
        }
        let unlisted = extensions.iter().find(|e| !is_allowed_extension(e));
        if let (Access::Write, Some(kind)) = (access, unlisted) {
            return Err(FileError::BadRequest(format!(
                "a plugin may not save a {kind} file: only plain data and picture kinds are allowed"
            )));
        }
        Ok(FileRequest {
            access,
            title: title.to_string(),
            extensions: extensions.iter().map(|e| e.to_string()).collect(),
        })
    }
}

/// The host's open and save dialogs. The interface implements this; tests use a stand-in.
pub trait FilePicker {
    /// Shows a dialog saying `plugin_name` asks for a file, and returns what the user chose,
    /// or `None` if they cancelled. For [`Access::Write`] the dialog confirms overwriting.
    fn pick(&mut self, plugin_name: &str, request: &FileRequest) -> Option<PathBuf>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    /// The plugin was not granted this capability at install.
    NotGranted(Capability),
    BadRequest(String),
    TooManyPicks,
    /// Not a handle the user gave this run.
    UnknownHandle,
    /// A read handle used to write, or a write handle used to read.
    WrongAccess,
    TooLarge,
    /// A file chosen to save as is written once; this one was already.
    AlreadyWritten,
    /// The chosen place is not one a plugin's file may be written to, or changed since it
    /// was chosen. The words never include the path.
    Refused(&'static str),
    /// The run was cancelled while its files were being written: none was changed.
    Cancelled,
    /// A file already written could not be taken back: its old contents, if it had any,
    /// are kept beside it in a `~pikslar-*.bak` file, never deleted.
    NotPutBack,
    /// The words never include the path.
    Io(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::NotGranted(c) => write!(f, "the plugin was not allowed to {}", c.word()),
            FileError::BadRequest(why) => f.write_str(why),
            FileError::TooManyPicks => {
                write!(f, "a run may ask for at most {MOST_PICKS_PER_RUN} files")
            }
            FileError::UnknownHandle => f.write_str("that is not a file the user chose"),
            FileError::WrongAccess => {
                f.write_str("that file was chosen for the other kind of access")
            }
            FileError::TooLarge => write!(f, "the file is larger than {MOST_FILE_BYTES} bytes"),
            FileError::AlreadyWritten => {
                f.write_str("a file chosen to save as is written once, and this one was")
            }
            FileError::Refused(why) => f.write_str(why),
            FileError::Cancelled => f.write_str("the run was cancelled; no file was written"),
            FileError::NotPutBack => f.write_str(
                "a file already written could not be taken back; its old contents are kept \
                 beside it, in a file whose name starts with ~pikslar- and ends with .bak",
            ),
            FileError::Io(why) => f.write_str(why),
        }
    }
}

/// One file the user chose.
struct Picked {
    access: Access,
    path: PathBuf,
    /// For a file to save as: what was there when it was chosen.
    seen: Seen,
    /// For a file to save as: a write was asked for. One per handle.
    claimed: bool,
}

/// The files the user chose during one run of one plugin. Made at the start of the run and
/// dropped at its end, so no handle outlives it.
pub struct FileGrants {
    plugin_name: String,
    granted: CapabilitySet,
    picked: Vec<Picked>,
    disk: Box<dyn Disk>,
    /// Folders no write may go in or under: where Pikslar keeps its settings and plugins.
    protected: Vec<PathBuf>,
}

impl fmt::Debug for FileGrants {
    /// Never prints a path.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileGrants")
            .field("plugin_name", &self.plugin_name)
            .field("granted", &self.granted)
            .field("picked", &self.picked.len())
            .finish()
    }
}

impl FileGrants {
    /// Writes go through [`PlainDisk`]; see [`FileGrants::with_disk`].
    pub fn for_run(plugin_name: &str, granted: CapabilitySet) -> FileGrants {
        FileGrants {
            plugin_name: plugin_name.to_string(),
            granted,
            picked: Vec::new(),
            disk: Box::new(PlainDisk),
            protected: Vec::new(),
        }
    }

    /// The same grants, refusing to write in or under any of `folders`, compared by where
    /// they really are (links resolved). The app gives its settings and plugins folders.
    pub fn with_protected(self, folders: Vec<PathBuf>) -> FileGrants {
        FileGrants {
            protected: folders,
            ..self
        }
    }

    /// The same grants, writing through `disk`: the app gives the system's replace.
    pub fn with_disk(self, disk: Box<dyn Disk>) -> FileGrants {
        FileGrants { disk, ..self }
    }

    /// Asks the user, through the host's dialog. `Ok(None)` when they cancel.
    #[inline(never)]
    pub fn pick(
        &mut self,
        picker: &mut dyn FilePicker,
        request: &FileRequest,
    ) -> Result<Option<FileHandle>, FileError> {
        let needed = request.access.capability();
        if !self.granted.contains(needed) {
            return Err(FileError::NotGranted(needed));
        }
        if self.picked.len() >= MOST_PICKS_PER_RUN {
            return Err(FileError::TooManyPicks);
        }
        let Some(path) = picker.pick(&self.plugin_name, request) else {
            return Ok(None);
        };
        let number = u32::try_from(self.picked.len()).map_err(|_| FileError::TooManyPicks)?;
        let seen = match request.access {
            Access::Read => Seen::Unusable,
            Access::Write => save::look(&path),
        };
        self.picked.push(Picked {
            access: request.access,
            path,
            seen,
            claimed: false,
        });
        Ok(Some(FileHandle(number)))
    }

    fn picked(&self, handle: FileHandle) -> Result<&Picked, FileError> {
        usize::try_from(handle.0)
            .ok()
            .and_then(|i| self.picked.get(i))
            .ok_or(FileError::UnknownHandle)
    }

    fn path(&self, handle: FileHandle, access: Access) -> Result<&Path, FileError> {
        let picked = self.picked(handle)?;
        match picked.access == access {
            true => Ok(&picked.path),
            false => Err(FileError::WrongAccess),
        }
    }

    /// The chosen file's name, without its folder, for a message. A name that would not show
    /// as itself (control or direction characters) is given as "the chosen file".
    #[inline(never)]
    pub fn file_name(&self, handle: FileHandle) -> Result<String, FileError> {
        let name = self
            .path(handle, Access::Read)?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match check_display("file name", &name, 255) {
            Ok(()) => Ok(name),
            Err(_) => Ok("the chosen file".to_string()),
        }
    }

    /// The whole file the user chose to be read.
    #[inline(never)]
    pub fn read(&self, handle: FileHandle) -> Result<Vec<u8>, FileError> {
        self.read_at_most(handle, MOST_FILE_BYTES)
    }

    /// How long the file the user chose to be read is now, from its metadata, without
    /// reading it: for refusing a read before it is made.
    #[inline(never)]
    pub fn len(&self, handle: FileHandle) -> Result<u64, FileError> {
        let (_, meta) = self.open_to_read(handle)?;
        Ok(meta.len())
    }

    /// The whole file the user chose to be read, refused as too large, before any byte is
    /// read, when it is longer than `most` bytes (or than [`MOST_FILE_BYTES`]); and refused
    /// as well if it has grown past that by the time it is read.
    #[inline(never)]
    pub fn read_at_most(&self, handle: FileHandle, most: u64) -> Result<Vec<u8>, FileError> {
        let mut reading = self.start_read(handle, most)?;
        let mut bytes = Vec::new();
        while !reading.read_some(&mut bytes, usize::MAX)? {}
        Ok(bytes)
    }

    /// The file the user chose to be read, opened to be read a piece at a time, so the caller
    /// can stop between pieces: refused as too large, before any byte is read, when it is
    /// longer than `most` bytes (or than [`MOST_FILE_BYTES`]).
    #[inline(never)]
    pub fn start_read(&self, handle: FileHandle, most: u64) -> Result<Reading, FileError> {
        let most = most.min(MOST_FILE_BYTES);
        let (file, meta) = self.open_to_read(handle)?;
        match meta.len() > most {
            true => Err(FileError::TooLarge),
            false => Ok(Reading {
                file: file.take(most + 1),
                len: meta.len(),
                most,
                read: 0,
            }),
        }
    }

    /// The file the user chose to be read, opened, with its metadata: refused when it is not
    /// a file or is larger than [`MOST_FILE_BYTES`].
    fn open_to_read(&self, handle: FileHandle) -> Result<(fs::File, fs::Metadata), FileError> {
        let path = self.path(handle, Access::Read)?;
        let file = fs::File::open(path).map_err(|e| plain_io("read the file", &e))?;
        let meta = file.metadata().map_err(|e| plain_io("read the file", &e))?;
        if !meta.is_file() {
            return Err(FileError::Io("the chosen item is not a file".to_string()));
        }
        match meta.len() > MOST_FILE_BYTES {
            true => Err(FileError::TooLarge),
            false => Ok((file, meta)),
        }
    }

    /// Takes the one write a file chosen to save as may have, of `len` bytes, before anything
    /// is written: refused for a read handle, a handle already written, or too many bytes.
    /// The bytes are given to [`FileGrants::commit`] when the run has succeeded.
    #[inline(never)]
    pub fn claim_write(&mut self, handle: FileHandle, len: u64) -> Result<(), FileError> {
        self.path(handle, Access::Write)?;
        if len > MOST_FILE_BYTES {
            return Err(FileError::TooLarge);
        }
        let picked = usize::try_from(handle.0)
            .ok()
            .and_then(|i| self.picked.get_mut(i))
            .ok_or(FileError::UnknownHandle)?;
        match std::mem::replace(&mut picked.claimed, true) {
            true => Err(FileError::AlreadyWritten),
            false => Ok(()),
        }
    }

    /// Writes every claimed file, all or none (see [`crate::save`]): each through a new
    /// temporary file beside it, then put in its place. `go_on` is asked between files,
    /// between the pieces of each, and before any is put in place; false cancels, leaving
    /// every file as it was.
    #[inline(never)]
    pub fn commit(
        &mut self,
        writes: &[(FileHandle, &[u8])],
        go_on: &dyn Fn() -> bool,
    ) -> Result<(), FileError> {
        let wanted = writes
            .iter()
            .map(|&(handle, bytes)| {
                let picked = usize::try_from(handle.0)
                    .ok()
                    .and_then(|i| self.picked.get(i))
                    .ok_or(FileError::UnknownHandle)?;
                match (picked.access, picked.claimed) {
                    (Access::Write, true) => Ok(Wanted {
                        target: &picked.path,
                        seen: picked.seen,
                        bytes,
                    }),
                    (Access::Write, false) => Err(FileError::UnknownHandle),
                    (Access::Read, _) => Err(FileError::WrongAccess),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        save::commit(&wanted, self.disk.as_mut(), &self.protected, go_on)
    }
}

/// A file the user chose, being read a piece at a time.
pub struct Reading {
    file: std::io::Take<fs::File>,
    len: u64,
    most: u64,
    read: u64,
}

impl fmt::Debug for Reading {
    /// Never prints a path or a handle.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reading")
            .field("len", &self.len)
            .field("read", &self.read)
            .finish()
    }
}

impl Reading {
    /// The file's length when it was opened.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reads at most `piece` more bytes onto `into`. True once the whole file is read;
    /// refused as too large if it has grown past the most it may hold.
    #[inline(never)]
    pub fn read_some(&mut self, into: &mut Vec<u8>, piece: usize) -> Result<bool, FileError> {
        let piece = u64::try_from(piece).unwrap_or(u64::MAX);
        let got = (&mut self.file)
            .take(piece)
            .read_to_end(into)
            .map_err(|e| plain_io("read the file", &e))?;
        self.read += got as u64;
        if self.read > self.most {
            return Err(FileError::TooLarge);
        }
        Ok((got as u64) < piece)
    }
}

/// An I/O failure in words without the path, which the plugin must not learn.
fn plain_io(doing: &str, e: &std::io::Error) -> FileError {
    FileError::Io(format!("could not {doing}: {}", e.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Answers(Vec<Option<PathBuf>>, Vec<String>);

    impl FilePicker for Answers {
        fn pick(&mut self, plugin_name: &str, request: &FileRequest) -> Option<PathBuf> {
            self.1.push(format!("{plugin_name}: {}", request.title));
            self.0.pop().flatten()
        }
    }

    fn temp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pikslar-files-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp");
        dir
    }

    fn both() -> CapabilitySet {
        CapabilitySet::NONE
            .with(Capability::PickFileToRead)
            .with(Capability::PickFileToWrite)
    }

    #[test]
    fn a_picked_file_is_read_and_written_only_through_its_handle() {
        let dir = temp("rw");
        fs::write(dir.join("in.txt"), "hello").expect("write");
        let mut picker = Answers(
            vec![Some(dir.join("out.txt")), Some(dir.join("in.txt"))],
            vec![],
        );
        let mut grants = FileGrants::for_run("Outline", both());
        let read = FileRequest::new(Access::Read, "Choose a palette", &["txt"]).expect("req");
        let write = FileRequest::new(Access::Write, "Save the report", &[]).expect("req");
        let r = grants
            .pick(&mut picker, &read)
            .expect("pick")
            .expect("chosen");
        let w = grants
            .pick(&mut picker, &write)
            .expect("pick")
            .expect("chosen");
        assert_eq!(
            picker.1,
            ["Outline: Choose a palette", "Outline: Save the report"]
        );
        assert_eq!(grants.read(r).expect("read"), b"hello");
        grants.claim_write(w, 4).expect("claimed");
        assert!(
            !dir.join("out.txt").exists(),
            "nothing written before the commit"
        );
        grants.commit(&[(w, b"done")], &|| true).expect("write");
        assert_eq!(fs::read(dir.join("out.txt")).expect("out"), b"done");
        assert_eq!(grants.read(w), Err(FileError::WrongAccess));
        assert_eq!(grants.file_name(w), Err(FileError::WrongAccess));
        assert_eq!(grants.file_name(r).as_deref(), Ok("in.txt"));
        assert_eq!(grants.claim_write(r, 1), Err(FileError::WrongAccess));
        assert_eq!(
            grants.commit(&[(r, b"x")], &|| true),
            Err(FileError::WrongAccess)
        );
        assert_eq!(grants.claim_write(w, 1), Err(FileError::AlreadyWritten));
        assert_eq!(
            grants.read(FileHandle::from_number(7)),
            Err(FileError::UnknownHandle)
        );
        assert_eq!(grants.file_name(r).expect("name"), "in.txt");
        assert!(!format!("{grants:?}").contains("in.txt"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_read_can_be_held_to_fewer_bytes_known_before_it_reads() {
        let dir = temp("most");
        fs::write(dir.join("in.txt"), "hello").expect("write");
        let mut picker = Answers(vec![Some(dir.join("in.txt"))], vec![]);
        let mut grants = FileGrants::for_run("Outline", both());
        let read = FileRequest::new(Access::Read, "Choose", &[]).expect("req");
        let r = grants
            .pick(&mut picker, &read)
            .expect("pick")
            .expect("chosen");
        assert_eq!(grants.len(r), Ok(5));
        assert_eq!(grants.read_at_most(r, 4), Err(FileError::TooLarge));
        assert_eq!(grants.read_at_most(r, 5).expect("read"), b"hello");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn without_the_capability_no_dialog_is_shown() {
        let mut picker = Answers(vec![Some(PathBuf::from("x"))], vec![]);
        let read_only = CapabilitySet::NONE.with(Capability::PickFileToRead);
        let mut grants = FileGrants::for_run("P", read_only);
        let write = FileRequest::new(Access::Write, "Save", &[]).expect("req");
        assert_eq!(
            grants.pick(&mut picker, &write),
            Err(FileError::NotGranted(Capability::PickFileToWrite))
        );
        let mut none = FileGrants::for_run("P", CapabilitySet::NONE.with(Capability::EditDocument));
        let read = FileRequest::new(Access::Read, "Open", &[]).expect("req");
        assert!(none.pick(&mut picker, &read).is_err());
        assert!(picker.1.is_empty(), "a dialog was shown");
    }

    #[test]
    fn cancelling_gives_no_handle_and_picks_are_limited() {
        let mut picker = Answers(vec![None; 20], vec![]);
        let mut grants = FileGrants::for_run("P", both());
        let read = FileRequest::new(Access::Read, "Open", &[]).expect("req");
        assert_eq!(grants.pick(&mut picker, &read), Ok(None));
        let mut picker = Answers(vec![Some(PathBuf::from("a")); 20], vec![]);
        for _ in 0..MOST_PICKS_PER_RUN {
            assert!(grants.pick(&mut picker, &read).expect("pick").is_some());
        }
        assert_eq!(
            grants.pick(&mut picker, &read),
            Err(FileError::TooManyPicks)
        );
    }

    #[test]
    fn requests_are_checked_and_errors_hold_no_path() {
        assert!(FileRequest::new(Access::Read, "", &[]).is_err());
        assert!(FileRequest::new(Access::Read, "Open\u{202E}gnp.exe", &[]).is_err());
        assert!(FileRequest::new(Access::Read, "Open", &["../x"]).is_err());
        assert!(FileRequest::new(Access::Read, "Open", &["PNG"]).is_err());
        let kept = FileError::NotPutBack.to_string();
        assert!(
            kept.contains("kept beside it") && !kept.contains("  "),
            "{kept}"
        );
        for kind in [
            "bat", "cmd", "exe", "ps1", "js", "vbs", "hta", "reg", "url", "lnk", "pyw", "py", "sh",
            "rdp", "theme", "wsb", "msu", "msix", "appx", "cer", "svg", "html", "hlp", "diagpkg",
            "ini",
        ] {
            let asked = FileRequest::new(Access::Write, "Save", &["txt", kind]);
            assert!(asked.is_err(), "{kind} offered to save as");
            assert!(FileRequest::new(Access::Read, "Open", &[kind]).is_ok());
        }
        let dir = temp("missing");
        let mut picker = Answers(vec![Some(dir.join("secret-name.txt"))], vec![]);
        let mut grants = FileGrants::for_run("P", both());
        let read = FileRequest::new(Access::Read, "Open", &[]).expect("req");
        let h = grants
            .pick(&mut picker, &read)
            .expect("pick")
            .expect("chosen");
        let why = grants.read(h).expect_err("missing").to_string();
        assert!(
            !why.contains("secret-name") && !why.contains("pikslar-files"),
            "{why}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
