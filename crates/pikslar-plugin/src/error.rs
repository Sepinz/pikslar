//! Why something about a plugin was refused, in words for the user.

use crate::manifest::{ManifestError, Version};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginError {
    /// Plugins are off: Pikslar was started with `--no-settings`, or has no settings folder.
    NoStore,
    /// The folder offered is not a plugin Pikslar takes.
    Package(String),
    Manifest(ManifestError),
    /// Reading or writing failed; the words say what was being done.
    Io(String),
    /// A newer version is installed. Uninstall it first to go back.
    Downgrade {
        installed: Version,
        offered: Version,
    },
    /// The installed copy has this version with other contents: a new release needs a new
    /// version number.
    SameVersionChanged {
        version: Version,
    },
    /// What is installed changed between the review and the approval: review again.
    ChangedSinceReview,
    NotInstalled(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::NoStore => write!(f, "plugins are off because settings are off"),
            PluginError::Package(why) => write!(f, "this is not a plugin Pikslar can install: {why}"),
            PluginError::Manifest(e) => write!(f, "this is not a plugin Pikslar can install: {e}"),
            PluginError::Io(why) => f.write_str(why),
            PluginError::Downgrade { installed, offered } => write!(
                f,
                "version {installed} is installed, newer than {offered}; uninstall it first to go back"
            ),
            PluginError::SameVersionChanged { version } => write!(
                f,
                "version {version} is installed with different files; a changed plugin needs a new version number"
            ),
            PluginError::ChangedSinceReview => write!(
                f,
                "the installed plugins changed since this was reviewed; review it again"
            ),
            PluginError::NotInstalled(id) => write!(f, "no plugin {id:?} is installed"),
        }
    }
}

impl std::error::Error for PluginError {}

/// An I/O failure with what was being done, never quietly dropped.
pub(crate) fn io(doing: &str, e: &std::io::Error) -> PluginError {
    PluginError::Io(format!("could not {doing}: {e}"))
}
