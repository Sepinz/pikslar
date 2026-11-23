//! The frozen files written by earlier versions of Pikslar.

use std::path::PathBuf;

/// One line of the manifest: what a frozen file must still turn into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: u32,
    pub width: usize,
    pub height: usize,
    pub layers: usize,
    pub frames: usize,
    pub tags: usize,
    pub floating: bool,
    pub hashes: Vec<u32>,
}

impl Entry {
    fn parse(line: &str) -> Option<Entry> {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [name, version, width, height, layers, frames, tags, floating, hashes] = fields[..]
        else {
            return None;
        };
        Some(Entry {
            name: name.to_owned(),
            version: version.parse().ok()?,
            width: width.parse().ok()?,
            height: height.parse().ok()?,
            layers: layers.parse().ok()?,
            frames: frames.parse().ok()?,
            tags: tags.parse().ok()?,
            floating: floating == "1",
            hashes: hashes
                .split(',')
                .map(|h| u32::from_str_radix(h, 16).ok())
                .collect::<Option<_>>()?,
        })
    }

    /// The bytes of the frozen file.
    pub fn bytes(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(dir().join(&self.name))
    }
}

/// Where the frozen files live.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// Every frozen file. Fails loudly: a corpus that cannot be read proves nothing.
pub fn entries() -> Result<Vec<Entry>, String> {
    let path = dir().join("manifest.txt");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Entry::parse(line).ok_or_else(|| format!("bad manifest line: {line}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_line_reads_back() {
        let entry = Entry::parse("a.piks 2 3 4 5 6 7 1 0000000a,ff").expect("valid");
        assert_eq!(entry.name, "a.piks");
        assert_eq!((entry.version, entry.width, entry.height), (2, 3, 4));
        assert_eq!((entry.layers, entry.frames, entry.tags), (5, 6, 7));
        assert!(entry.floating);
        assert_eq!(entry.hashes, vec![10, 255]);
    }

    #[test]
    fn a_short_or_garbled_line_is_refused() {
        assert_eq!(Entry::parse("a.piks 2 3"), None);
        assert_eq!(Entry::parse("a.piks x 3 4 5 6 7 1 aa"), None);
        assert_eq!(Entry::parse("a.piks 2 3 4 5 6 7 1 zz"), None);
    }
}
