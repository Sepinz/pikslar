//! What the user is shown before a plugin is installed, and the pending install that only an
//! explicit approval turns into files.

use crate::capability::{Capability, CapabilitySet};
use crate::manifest::Version;
use crate::package::Package;
use crate::sha256::hex;

/// How an install relates to what is already there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Nothing with this id is installed.
    Fresh,
    /// An older version is installed and will be replaced. Its name and author are kept so
    /// the review can warn when a plugin takes over an id that another one used.
    Upgrade {
        from: Version,
        previous_name: String,
        previous_author: Option<String>,
    },
    /// The same version with the same files is installed: installing again changes nothing
    /// but the approval.
    Reinstall,
    /// A copy with this id is installed but disabled, for this reason; it will be replaced.
    Repair { reason: String },
}

/// One capability as the review shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityLine {
    pub capability: Capability,
    /// [`Capability::plain_words`].
    pub words: &'static str,
    /// True when an upgrade asks for this and the installed version did not.
    pub new: bool,
}

/// Everything the install dialog shows. Made from the package alone; nothing has run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReview {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub author: Option<String>,
    pub description: Option<String>,
    /// The scripting API version the plugin was written for.
    pub api: u32,
    pub capabilities: Vec<CapabilityLine>,
    /// The menu titles of the commands the plugin adds.
    pub commands: Vec<String>,
    pub change: Change,
    pub files: usize,
    pub bytes: u64,
    /// Lower-case hex SHA-256 of the content, for anyone comparing with a published value.
    pub hash: String,
}

/// What the review says the fingerprint is for, and what it is not. A content hash shows
/// whether files changed; with no signature, it says nothing about who made them.
pub const FINGERPRINT_MEANS: &str = "The fingerprint detects accidental or unexpected changes \
     to these files. It does not prove who made the plugin, or stop a program that can change \
     your files.";

/// A lower-case hex hash as the review and the plugin's menu show it: groups of 8, so it can
/// be read out and compared with a published value.
#[inline(never)]
pub fn fingerprint(hash: &str) -> String {
    let chars: Vec<char> = hash.chars().collect();
    let groups: Vec<String> = chars.chunks(8).map(|g| g.iter().collect()).collect();
    groups.join(" ")
}

/// The author line: the plugin's own claim, marked as unverified, since nothing checks it.
#[inline(never)]
pub fn author_line(author: Option<&str>) -> String {
    match author {
        Some(a) => format!("Author: {a} (unverified: as the plugin states it)"),
        None => "Author: not stated".to_string(),
    }
}

impl InstallReview {
    pub(crate) fn new(package: &Package, change: Change, before: CapabilitySet) -> InstallReview {
        let m = package.manifest();
        // Only an upgrade marks capabilities as new: for a fresh install or a repair, all are.
        let added = match change {
            Change::Upgrade { .. } => m.capabilities.added_since(before),
            _ => CapabilitySet::NONE,
        };
        InstallReview {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version,
            author: m.author.clone(),
            description: m.description.clone(),
            api: m.api,
            capabilities: m
                .capabilities
                .iter()
                .map(|c| CapabilityLine {
                    capability: c,
                    words: c.plain_words(),
                    new: added.contains(c),
                })
                .collect(),
            commands: m.commands.iter().map(|c| c.title.clone()).collect(),
            change,
            files: package.files().len(),
            bytes: package.total_bytes(),
            hash: hex(&package.hash()),
        }
    }

    /// The review as lines of plain text, in the order a dialog shows them: who it is (name,
    /// version and the ASCII id), then everything it asks for, then the rest (the author,
    /// marked as unverified, and the content fingerprint with what it does and does not
    /// show), with the plugin's own description last, so that a long description can never
    /// push what it asks for out of sight.
    #[inline(never)]
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            format!("{} {}", self.name, self.version),
            format!("Id: {}. Scripting API {}.", self.id, self.api),
        ];
        match self.capabilities.is_empty() {
            true => out.push("It asks for nothing: it cannot see or change your work.".to_string()),
            false => out.push("It asks to:".to_string()),
        }
        for line in &self.capabilities {
            let mark = if line.new {
                " (new in this version)"
            } else {
                ""
            };
            out.push(format!("- {}{mark}", line.words));
        }
        out.push(format!("It adds: {}.", self.commands.join(", ")));
        out.push("It cannot open files or folders by itself, or use the network.".to_string());
        out.push(self.change_line());
        out.push(author_line(self.author.as_deref()));
        out.push(format!(
            "Fingerprint (SHA-256 of its files): {}",
            fingerprint(&self.hash)
        ));
        out.push(FINGERPRINT_MEANS.to_string());
        out.extend(self.description_line());
        out
    }

    /// The line of [`InstallReview::lines`] that holds the plugin's own description, if it
    /// has one: the last line.
    #[inline(never)]
    pub fn description_line(&self) -> Option<String> {
        self.description
            .as_ref()
            .map(|d| format!("Its own description: {d}"))
    }

    fn change_line(&self) -> String {
        match &self.change {
            Change::Fresh => "This is a new plugin.".to_string(),
            Change::Upgrade {
                from,
                previous_name,
                previous_author,
            } => match *previous_name == self.name && *previous_author == self.author {
                true => format!("This replaces version {from}."),
                false => format!(
                    "Warning: this replaces version {from} of a plugin called {previous_name:?}{}, \
                     which had the same id. Install it only if you expected that.",
                    previous_author
                        .as_ref()
                        .map_or(String::new(), |a| format!(" by {a}"))
                ),
            },
            Change::Reinstall => "This version is already installed.".to_string(),
            Change::Repair { reason } => {
                format!("This replaces an installed copy that is disabled: {reason}")
            }
        }
    }
}

/// What the installed folder for an id looked like at review time, so an approval is refused
/// if it changed meanwhile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Snapshot {
    Absent,
    /// SHA-256 of the record file's bytes, or of nothing when it had none.
    Present([u8; 32]),
    /// Something other than a plain file is where the record should be.
    NotPlain,
}

/// A reviewed package waiting for the user. Nothing has been written. Only
/// [`crate::PluginStore::approve`] installs it; dropping it, or [`PendingInstall::deny`],
/// leaves everything as it was.
#[derive(Debug)]
pub struct PendingInstall {
    pub(crate) package: Package,
    pub(crate) review: InstallReview,
    pub(crate) snapshot: Snapshot,
    /// The store folder it was reviewed for: another store refuses it.
    pub(crate) root: std::path::PathBuf,
}

impl PendingInstall {
    #[inline(never)]
    pub fn review(&self) -> &InstallReview {
        &self.review
    }

    /// The user said no. Nothing was written, so nothing is undone.
    pub fn deny(self) {}
}
