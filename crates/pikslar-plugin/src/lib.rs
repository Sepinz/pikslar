//! Plugins for Pikslar, without any interface and without running anything.
//!
//! A plugin is a flat folder holding `plugin.json` and the scripts its commands run. The
//! store reads it into memory, shows an [`InstallReview`], and installs only after
//! [`PluginStore::approve`]. Loading checks the files against the hash recorded at install,
//! so a changed plugin is disabled, never half-loaded. Plugins get no paths: files reach them
//! only through pickers the user answers ([`files`]).
//!
//! See `docs/plugins.md` for the format, the capabilities, the install flow and the threat
//! model.

mod approvals;
mod capability;
mod error;
pub mod files;
pub mod hidden;
mod identity;
mod install;
mod kinds;
mod manifest;
mod names;
mod package;
mod record;
mod reparse;
mod review;
mod rollback;
mod save;
mod sha256;
mod startup;
mod store;

pub use capability::{Capability, CapabilitySet};
pub use error::PluginError;
pub use manifest::{
    Command, Manifest, ManifestError, Version, MANIFEST_FORMAT, MOST_COMMANDS, MOST_MANIFEST_BYTES,
};
pub use names::{check_id, check_script_name, MANIFEST_FILE, SCRIPT_EXTENSION};
pub use package::{Package, PackageFile, MOST_FILES, MOST_PACKAGE_BYTES, MOST_SCRIPT_BYTES};
pub use record::RECORD_FILE;
pub use review::{
    author_line, fingerprint, CapabilityLine, Change, InstallReview, PendingInstall,
    FINGERPRINT_MEANS,
};
pub use sha256::{hex, sha256, Sha256};
pub use store::{Disabled, Listed, LoadedPlugin, PluginStore, MOST_LISTED, PLUGINS_FOLDER};
