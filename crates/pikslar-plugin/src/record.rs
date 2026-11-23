//! `installed.json`: what Pikslar recorded when the user approved a plugin. It is checked
//! against the files every time the plugin is loaded.

use crate::capability::{Capability, CapabilitySet};
use crate::manifest::Version;
use crate::names::{check_id, check_script_name, MANIFEST_FILE};
use crate::sha256::hex;
use pikslar_json::{parse, to_text, Limits, Value};

/// The record's name inside an installed plugin's folder. Not a name a package may use.
pub const RECORD_FILE: &str = "installed.json";
const RECORD_FORMAT: i64 = 1;
const LIMITS: Limits = Limits {
    depth: 2,
    string_chars: 128,
    array_items: 32,
    object_members: 8,
};
const KEYS: [&str; 6] = ["record", "id", "version", "hash", "capabilities", "files"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub id: String,
    pub version: Version,
    /// Lower-case hex SHA-256 of the package's content.
    pub hash: String,
    /// What the user approved.
    pub capabilities: CapabilitySet,
    /// The package's file names, sorted.
    pub files: Vec<String>,
}

impl Record {
    #[inline(never)]
    pub fn new(
        id: &str,
        version: Version,
        hash: &[u8; 32],
        capabilities: CapabilitySet,
        files: Vec<String>,
    ) -> Record {
        Record {
            id: id.to_string(),
            version,
            hash: hex(hash),
            capabilities,
            files,
        }
    }

    #[inline(never)]
    pub fn to_bytes(&self) -> Vec<u8> {
        let words = self
            .capabilities
            .iter()
            .map(|c| Value::Str(c.word().to_string()))
            .collect();
        let files = self.files.iter().map(|f| Value::Str(f.clone())).collect();
        let value = Value::Object(vec![
            ("record".to_string(), Value::Int(RECORD_FORMAT)),
            ("id".to_string(), Value::Str(self.id.clone())),
            ("version".to_string(), Value::Str(self.version.to_string())),
            ("hash".to_string(), Value::Str(self.hash.clone())),
            ("capabilities".to_string(), Value::Array(words)),
            ("files".to_string(), Value::Array(files)),
        ]);
        let mut text = to_text(&value);
        text.push('\n');
        text.into_bytes()
    }

    #[inline(never)]
    pub fn parse(bytes: &[u8]) -> Result<Record, String> {
        let value = parse(bytes, &LIMITS).map_err(|e| e.to_string())?;
        let Value::Object(m) = &value else {
            return Err("it is not an object".to_string());
        };
        if let Some((key, _)) = m.iter().find(|(k, _)| !KEYS.contains(&k.as_str())) {
            return Err(format!("it has the unknown key {key:?}"));
        }
        let get = |key: &str| m.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        if get("record") != Some(&Value::Int(RECORD_FORMAT)) {
            return Err("its format is not one this Pikslar reads".to_string());
        }
        let text = |key: &str| match get(key) {
            Some(Value::Str(s)) => Ok(s.as_str()),
            _ => Err(format!("{key:?} is missing or not text")),
        };
        let id = text("id")?;
        check_id(id)?;
        let hash = text("hash")?;
        if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err("its hash is not a SHA-256".to_string());
        }
        Ok(Record {
            id: id.to_string(),
            version: Version::parse(text("version")?)?,
            hash: hash.to_string(),
            capabilities: capabilities(get("capabilities"))?,
            files: files(get("files"))?,
        })
    }
}

fn words(value: Option<&Value>, what: &str) -> Result<Vec<String>, String> {
    let Some(Value::Array(items)) = value else {
        return Err(format!("{what:?} is missing or not a list"));
    };
    items
        .iter()
        .map(|v| match v {
            Value::Str(s) => Ok(s.clone()),
            _ => Err(format!("{what:?} holds something other than text")),
        })
        .collect()
}

fn capabilities(value: Option<&Value>) -> Result<CapabilitySet, String> {
    let mut set = CapabilitySet::NONE;
    for word in words(value, "capabilities")? {
        let c = Capability::from_word(&word)
            .ok_or_else(|| format!("the capability {word:?} is not one Pikslar knows"))?;
        set = set.with(c);
    }
    Ok(set)
}

fn files(value: Option<&Value>) -> Result<Vec<String>, String> {
    let names = words(value, "files")?;
    for name in &names {
        if name != MANIFEST_FILE {
            check_script_name(name)?;
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record::new(
            "com.example.outline",
            Version::parse("1.2.3").expect("version"),
            &[0xab; 32],
            CapabilitySet::NONE.with(Capability::EditDocument),
            vec!["outline.pks".to_string(), "plugin.json".to_string()],
        )
    }

    #[test]
    fn a_record_round_trips() {
        let r = record();
        assert_eq!(Record::parse(&r.to_bytes()), Ok(r));
    }

    #[test]
    fn a_damaged_record_is_refused() {
        let good = String::from_utf8(record().to_bytes()).expect("text");
        let hash = hex(&[0xab; 32]);
        for (from, to) in [
            ("\"record\":1", "\"record\":2"),
            ("com.example.outline", "../x"),
            (hash.as_str(), "abc"),
            (hash.as_str(), &hash.to_uppercase()),
            ("edit-document", "network"),
            ("outline.pks", "../outline.pks"),
            ("1.2.3", "1.2"),
            ("\"files\"", "\"more\""),
        ] {
            let bad = good.replacen(from, to, 1);
            assert_ne!(bad, good, "{from}");
            assert!(Record::parse(bad.as_bytes()).is_err(), "{bad}");
        }
        assert!(Record::parse(b"").is_err());
    }
}
