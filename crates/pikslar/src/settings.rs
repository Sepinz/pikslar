//! Settings kept between runs: a short text file in the user's own data folder, and nowhere
//! else. A missing, damaged or unreadable file never stops launch: whatever cannot be read
//! keeps its default, and a project is never touched.

use crate::shortcuts::Keymap;
use crate::workspace::model::{Panel, Preset, Slot, Workspaces};
use pikslar_render::{OnionColors, OnionStyle};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "settings.txt";
/// A settings file is a few hundred bytes. Anything far larger is not one.
const MOST_BYTES: u64 = 64 * 1024;
/// How many recently used files the Open menu offers.
pub(crate) const MOST_RECENT: usize = 8;
const HEADER: &str = "# Pikslar settings. A line that cannot be read keeps its default.";

/// Everything kept between runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    /// None follows the display; otherwise 1, 2 or 3.
    pub interface_size: Option<i32>,
    /// Interface words in the larger face: a choice, or none while the text size decides.
    pub large_text: Option<bool>,
    pub onion: OnionStyle,
    /// Newest first, at most [`MOST_RECENT`], no repeats.
    pub recent: Vec<PathBuf>,
    /// Only the shortcuts changed from their usual keys are written.
    pub keys: Keymap,
    /// What each script run is held to: host settings, not the document's.
    pub script: pikslar_bind::ScriptLimits,
    /// Rewind: undo history kept between sessions. Off unless turned on.
    pub rewind: bool,
    /// Local crash and saving diagnostics are on unless this is set (`docs/diagnostics.md`).
    pub no_diagnostics: bool,
    /// The workspaces (docs/ui-redesign.md 3.9).
    pub workspaces: Workspaces,
}

impl Settings {
    /// Reads what it can. Unknown names, bad values and repeats past the limit are skipped.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut onion = s.onion;
        let mut keys_read = Vec::new();
        let mut workspace_lines = Vec::new();
        let mut legacy_folds = Vec::new();
        // An editor may have added a byte order mark, which would hide the first name.
        for line in text.trim_start_matches('\u{feff}').lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            match name {
                "interface_size" => s.interface_size = parse_size(value).unwrap_or(None),
                "rewind" => s.rewind = parse_bool(value).unwrap_or(s.rewind),
                "diagnostics" => {
                    s.no_diagnostics = parse_bool(value).map_or(s.no_diagnostics, |on| !on);
                }
                "large_text" => s.large_text = parse_bool(value),
                "recent" if !value.is_empty() && s.recent.len() < MOST_RECENT => {
                    let path = PathBuf::from(value);
                    if !s.recent.contains(&path) {
                        s.recent.push(path);
                    }
                }
                _ if crate::workspace::persist::is_workspace_line(name) => {
                    workspace_lines.push((name, value));
                }
                _ if name.starts_with("key.") => s.keys.apply_line(name, value, &mut keys_read),
                _ if name.starts_with("script.") => {
                    s.script = crate::settings_script::parse(name, value, s.script);
                }
                _ if name.starts_with("fold.") => {
                    legacy_folds.extend(parse_fold(name, value));
                }
                _ => onion = parse_onion(name, value, onion),
            }
        }
        s.workspaces = crate::workspace::persist::read(&workspace_lines);
        if !workspace_lines
            .iter()
            .any(|(n, _)| n.starts_with("dock.draw."))
        {
            s.workspaces = with_legacy_folds(&s.workspaces, &legacy_folds);
        }
        // The skin never hides both sides: it would show nothing while switched on.
        if onion.past || onion.future {
            s.onion = onion;
        } else {
            s.onion = OnionStyle {
                past: true,
                future: true,
                ..onion
            };
        }
        s
    }

    pub fn to_text(&self) -> String {
        let size = self
            .interface_size
            .map_or("automatic".to_string(), |n| n.to_string());
        let mut lines = vec![HEADER.to_string(), format!("interface_size = {size}")];
        let o = self.onion;
        lines.push(format!("onion.past = {}", yes_no(o.past)));
        lines.push(format!("onion.future = {}", yes_no(o.future)));
        lines.push(format!("onion.strength = {}", o.strength));
        lines.push(format!("onion.colors = {}", colors_word(o.colors)));
        for path in &self.recent {
            lines.push(format!("recent = {}", path.display()));
        }
        lines.extend(self.keys.changed_lines());
        lines.extend(crate::settings_script::lines(self.script));
        lines.push(format!("rewind = {}", yes_no(self.rewind)));
        lines.push(format!("diagnostics = {}", yes_no(!self.no_diagnostics)));
        lines.push(format!(
            "large_text = {}",
            self.large_text.map_or("auto", yes_no)
        ));
        lines.extend(crate::workspace::persist::lines(&self.workspaces));
        lines.push(String::new());
        lines.join("\r\n")
    }
}

/// Puts `path` first in `recent`, dropping an older mention and the oldest past the limit.
pub(crate) fn remember(recent: &mut Vec<PathBuf>, path: &Path) {
    recent.retain(|p| p != path);
    recent.insert(0, path.to_path_buf());
    recent.truncate(MOST_RECENT);
}

/// `Some(None)` for automatic, `Some(Some(n))` for a size, `None` for anything else.
fn parse_size(value: &str) -> Option<Option<i32>> {
    match value {
        "automatic" => Some(None),
        _ => value
            .parse::<i32>()
            .ok()
            .filter(|n| (1..=pikslar_platform::LARGEST_SIZE).contains(n))
            .map(Some),
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn yes_no(on: bool) -> &'static str {
    if on {
        "yes"
    } else {
        "no"
    }
}

/// A fold line of the settings written before the workspaces: the panel, and whether its
/// section was folded.
fn parse_fold(name: &str, value: &str) -> Option<(Panel, bool)> {
    // Only the right panel's four sections ever folded before the docks.
    let panel = match name.strip_prefix("fold.")? {
        "color" => Panel::Color,
        "palette" => Panel::Palette,
        "layers" => Panel::Layers,
        "lint" => Panel::Lint,
        _ => return None,
    };
    Some((panel, parse_bool(value)?))
}

/// Draw's arrangement with the folds of older settings applied to the slots showing them.
fn with_legacy_folds(w: &Workspaces, folds: &[(Panel, bool)]) -> Workspaces {
    let Some(mut ws) = w.layout(Preset::Draw) else {
        return w.clone();
    };
    for &(panel, folded) in folds {
        if let Some((side, i, _)) = ws.find(panel) {
            ws = crate::dock::with_slot(ws, side, i, |sl| Slot {
                open: !folded,
                ..sl
            });
        }
    }
    w.with_layout(Preset::Draw, ws)
}

fn colors_word(colors: OnionColors) -> &'static str {
    match colors {
        OnionColors::RedBlue => "red-blue",
        OnionColors::GreenPurple => "green-purple",
        OnionColors::Own => "own",
    }
}

fn parse_onion(name: &str, value: &str, o: OnionStyle) -> OnionStyle {
    let on = parse_bool(value);
    match (name, on) {
        ("onion.past", Some(past)) => OnionStyle { past, ..o },
        ("onion.future", Some(future)) => OnionStyle { future, ..o },
        ("onion.strength", _) => match value.parse::<usize>() {
            Ok(n) if n < pikslar_render::onion::STRENGTHS.len() => OnionStyle { strength: n, ..o },
            _ => o,
        },
        ("onion.colors", _) => OnionColors::ALL
            .into_iter()
            .find(|&c| colors_word(c) == value)
            .map_or(o, |colors| OnionStyle { colors, ..o }),
        _ => o,
    }
}

/// Where the settings live, or nowhere: tests and measurement runs keep none.
#[derive(Clone, Debug, Default)]
pub(crate) struct SettingsFile {
    path: Option<PathBuf>,
}

impl SettingsFile {
    pub fn disabled() -> Self {
        SettingsFile::default()
    }

    pub fn in_dir(dir: &Path) -> Self {
        SettingsFile {
            path: Some(dir.join(FILE_NAME)),
        }
    }

    /// The usual per-user folder, the same one crash recovery uses.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("Pikslar"))
    }

    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    /// The settings folder, or none when settings are off.
    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// A file of this name in the settings folder, or none when settings are off.
    pub fn sibling(&self, name: &str) -> Option<PathBuf> {
        self.path.as_ref().map(|p| p.with_file_name(name))
    }

    /// The settings, with a problem worth mentioning when a file is there but could not be
    /// used. A missing file is not a problem.
    pub fn load(&self) -> (Settings, Option<String>) {
        let Some(path) = &self.path else {
            return (Settings::default(), None);
        };
        match read_limited(path) {
            Ok(Some(text)) => (Settings::parse(&text), None),
            Ok(None) => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(e.to_string())),
        }
    }

    /// Moves a file that could not be used to `settings.txt.bad`, replacing an older one, so
    /// the next save cannot write over it. False if it could not be moved.
    pub fn set_aside(&self) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        fs::rename(path, path.with_extension("txt.bad")).is_ok()
    }

    /// Writes beside the file and renames over it, so a crash mid-write leaves the old one.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let fresh = path.with_extension("txt.new");
        fs::write(&fresh, settings.to_text())?;
        fs::rename(&fresh, path)
    }
}

fn read_limited(path: &Path) -> io::Result<Option<String>> {
    let size = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if size > MOST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the settings file is {size} bytes, too large to be one"),
        ));
    }
    let bytes = fs::read(path)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
