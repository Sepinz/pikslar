//! `plugin.json`: what a plugin is, what it asks for and what it adds to the menus.
//!
//! Read strictly: a key Pikslar does not know, a key given twice, a missing key, a value of
//! the wrong kind, an oversized file or field, or an unknown capability refuses the plugin.

use crate::capability::{Capability, CapabilitySet};
use crate::names::{check_command_id, check_display, check_id, check_script_name};
use pikslar_json::{parse, Limits, Value};
use std::fmt;

/// The manifest format this reader takes, the value of its `"manifest"` key.
pub const MANIFEST_FORMAT: i64 = 1;
/// The largest `plugin.json`, in bytes.
pub const MOST_MANIFEST_BYTES: usize = 16 * 1024;
/// The most commands one plugin adds.
pub const MOST_COMMANDS: usize = 16;

const MOST_NAME_CHARS: usize = 48;
const MOST_TITLE_CHARS: usize = 48;
const MOST_AUTHOR_CHARS: usize = 64;
const MOST_DESCRIPTION_CHARS: usize = 400;
const MOST_VERSION_PART: u32 = 99_999;

const LIMITS: Limits = Limits {
    depth: 3,
    string_chars: 512,
    array_items: 16,
    object_members: 12,
};

const KEYS: [&str; 9] = [
    "manifest",
    "id",
    "name",
    "version",
    "api",
    "capabilities",
    "commands",
    "description",
    "author",
];
const COMMAND_KEYS: [&str; 3] = ["id", "title", "script"];

/// A plugin's version: three whole numbers, `major.minor.patch`, compared in that order.
/// No pre-release or build suffixes: every version is a release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Reads `1.2.3`: no leading zeros, no sign, no spaces, each part at most 99999.
    #[inline(never)]
    pub fn parse(text: &str) -> Result<Version, String> {
        let parts: Vec<&str> = text.split('.').collect();
        let numbers: Option<Vec<u32>> = parts.iter().map(|p| version_part(p)).collect();
        match (parts.len(), numbers) {
            (3, Some(n)) => Ok(Version {
                major: n[0],
                minor: n[1],
                patch: n[2],
            }),
            _ => Err(format!(
                "the version {text:?} must be three whole numbers like 1.4.0"
            )),
        }
    }
}

fn version_part(part: &str) -> Option<u32> {
    let digits = !part.is_empty() && part.len() <= 5 && part.bytes().all(|b| b.is_ascii_digit());
    let leading_zero = part.len() > 1 && part.starts_with('0');
    match digits && !leading_zero {
        true => part.parse().ok().filter(|&n| n <= MOST_VERSION_PART),
        false => None,
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// One menu command a plugin adds: running it runs `script`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    /// What the menu shows.
    pub title: String,
    /// A script file in the plugin folder.
    pub script: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    /// The scripting API version the scripts were written for.
    pub api: u32,
    pub capabilities: CapabilitySet,
    pub commands: Vec<Command>,
    pub description: Option<String>,
    pub author: Option<String>,
}

/// Why a manifest was refused, in words for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin.json: {}", self.0)
    }
}

impl Manifest {
    /// Reads and checks a manifest. The API version is checked against what this Pikslar
    /// supports, so an install never shows a plugin that could not run.
    #[inline(never)]
    pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
        if bytes.len() > MOST_MANIFEST_BYTES {
            return Err(ManifestError(format!(
                "the file is {} bytes, more than the {MOST_MANIFEST_BYTES} allowed",
                bytes.len()
            )));
        }
        let value = parse(bytes, &LIMITS).map_err(|e| ManifestError(e.to_string()))?;
        read_manifest(&value).map_err(ManifestError)
    }

    #[inline(never)]
    pub fn command(&self, id: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.id == id)
    }
}

type Members = [(String, Value)];

fn members<'a>(value: &'a Value, what: &str, allowed: &[&str]) -> Result<&'a Members, String> {
    let Value::Object(members) = value else {
        return Err(format!("{what} must be an object, not {}", value.kind()));
    };
    match members.iter().find(|(k, _)| !allowed.contains(&k.as_str())) {
        Some((key, _)) => Err(format!(
            "{what} has the key {key:?}, which Pikslar does not know"
        )),
        None => Ok(members),
    }
}

fn get<'a>(members: &'a Members, key: &str) -> Option<&'a Value> {
    members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn text<'a>(members: &'a Members, key: &str) -> Result<&'a str, String> {
    match get(members, key) {
        Some(Value::Str(s)) => Ok(s),
        Some(other) => Err(format!("{key:?} must be a string, not {}", other.kind())),
        None => Err(format!("the key {key:?} is missing")),
    }
}

fn optional_text(
    members: &Members,
    key: &str,
    most_chars: usize,
) -> Result<Option<String>, String> {
    match get(members, key) {
        None => Ok(None),
        Some(_) => {
            let s = text(members, key)?;
            check_display(key, s, most_chars)?;
            Ok(Some(s.to_string()))
        }
    }
}

fn whole(members: &Members, key: &str) -> Result<i64, String> {
    match get(members, key) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!(
            "{key:?} must be a whole number, not {}",
            other.kind()
        )),
        None => Err(format!("the key {key:?} is missing")),
    }
}

fn read_manifest(value: &Value) -> Result<Manifest, String> {
    let m = members(value, "the manifest", &KEYS)?;
    let format = whole(m, "manifest")?;
    if format != MANIFEST_FORMAT {
        return Err(format!(
            "manifest format {format} is not one this Pikslar reads (it reads {MANIFEST_FORMAT})"
        ));
    }
    let id = text(m, "id")?;
    check_id(id)?;
    let name = text(m, "name")?;
    check_display("name", name, MOST_NAME_CHARS)?;
    let version = Version::parse(text(m, "version")?)?;
    let api = read_api(whole(m, "api")?)?;
    let capabilities = read_capabilities(get(m, "capabilities"))?;
    let commands = read_commands(get(m, "commands"))?;
    Ok(Manifest {
        id: id.to_string(),
        name: name.to_string(),
        version,
        api,
        capabilities,
        commands,
        description: optional_text(m, "description", MOST_DESCRIPTION_CHARS)?,
        author: optional_text(m, "author", MOST_AUTHOR_CHARS)?,
    })
}

fn read_api(api: i64) -> Result<u32, String> {
    let supported = pikslar_api::API_VERSION;
    match u32::try_from(api) {
        Ok(v) if v == supported => Ok(v),
        _ => Err(format!(
            "the plugin needs scripting API {api}, and this Pikslar supports only {supported}"
        )),
    }
}

fn read_capabilities(value: Option<&Value>) -> Result<CapabilitySet, String> {
    let Some(Value::Array(items)) = value else {
        return Err("\"capabilities\" must be a list of words, empty for none".to_string());
    };
    let mut set = CapabilitySet::NONE;
    for item in items {
        let Value::Str(word) = item else {
            return Err(format!("a capability must be a word, not {}", item.kind()));
        };
        let c = Capability::from_word(word)
            .ok_or_else(|| format!("the capability {word:?} is not one Pikslar knows"))?;
        if set.contains(c) {
            return Err(format!("the capability {word:?} is listed twice"));
        }
        set = set.with(c);
    }
    Ok(set)
}

fn read_commands(value: Option<&Value>) -> Result<Vec<Command>, String> {
    let Some(Value::Array(items)) = value else {
        return Err("\"commands\" must be a list of commands".to_string());
    };
    if items.is_empty() || items.len() > MOST_COMMANDS {
        return Err(format!("a plugin adds 1 to {MOST_COMMANDS} commands"));
    }
    let mut commands: Vec<Command> = Vec::with_capacity(items.len());
    for item in items {
        let c = read_command(item)?;
        if commands.iter().any(|o| o.id == c.id) {
            return Err(format!("the command id {:?} is used twice", c.id));
        }
        if commands.iter().any(|o| o.title == c.title) {
            return Err(format!("the command title {:?} is used twice", c.title));
        }
        commands.push(c);
    }
    Ok(commands)
}

fn read_command(value: &Value) -> Result<Command, String> {
    let m = members(value, "a command", &COMMAND_KEYS)?;
    let id = text(m, "id")?;
    check_command_id(id)?;
    let title = text(m, "title")?;
    check_display("command title", title, MOST_TITLE_CHARS)?;
    let script = text(m, "script")?;
    check_script_name(script)?;
    Ok(Command {
        id: id.to_string(),
        title: title.to_string(),
        script: script.to_string(),
    })
}
