//! Repository data versions and ordered migrations of the shared configuration.

use std::{fmt, str::FromStr};

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use toml_edit::ImDocument;

use crate::{Error, Result};

/// Configuration interpretation version; governed separately from stored data.
pub const FORMAT_VERSION: u32 = 1;

/// Supported source formats for both public and private repository data.
/// Individual Markdown documents and disposable caches have no data version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DataVersion {
    /// Legacy format, also used when the shared field is absent.
    V1Alpha1,
    /// Explicit repository-wide version marker; Markdown layout is unchanged.
    V1Alpha2,
}

/// Version used when the shared configuration has no `data_version` field.
pub const LEGACY: DataVersion = DataVersion::V1Alpha1;
/// Version written by new stores and the target of an explicit upgrade.
pub const CURRENT: DataVersion = DataVersion::V1Alpha2;
/// All readable and writable data versions, in migration order.
pub const SUPPORTED: [DataVersion; 2] = [LEGACY, CURRENT];

impl DataVersion {
    /// Stable spelling used in TOML, JSON, and notices.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1Alpha1 => "v1alpha1",
            Self::V1Alpha2 => "v1alpha2",
        }
    }

    /// Whether an explicit upgrade can move this version to the current format.
    pub fn upgrade_available(self) -> bool {
        self != CURRENT
    }
}

impl fmt::Display for DataVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DataVersion {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        SUPPORTED
            .into_iter()
            .find(|v| v.as_str() == value)
            .ok_or_else(|| {
                Error::new(
                    "configuration",
                    format!(
                        "Unsupported data_version {value:?}; supported versions: {}",
                        SUPPORTED.map(Self::as_str).join(", ")
                    ),
                )
            })
    }
}

/// Result of an explicit store upgrade. A preview never writes sources.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpgradeResult {
    /// Version read while holding the operation lock.
    pub source_version: DataVersion,
    /// Application's current data version.
    pub target_version: DataVersion,
    /// True only when this call saved the migrated shared configuration.
    pub wrote: bool,
    /// Whether the caller requested validation without a source save.
    pub dry_run: bool,
}

struct Migration {
    source: DataVersion,
    target: DataVersion,
}

// Each step changes only the shared version field. Future data changes belong
// in explicit steps here rather than in initialization or cache recovery.
const MIGRATIONS: &[Migration] = &[Migration {
    source: LEGACY,
    target: CURRENT,
}];

pub(crate) fn migrate(text: &str, source: DataVersion) -> Result<String> {
    let document = ImDocument::parse(text)
        .map_err(|e| Error::new("configuration", format!("Invalid store config.toml: {e}")))?;
    let mut version = source;
    for step in MIGRATIONS {
        if step.source == version {
            version = step.target;
        }
    }
    let value = toml_edit::Value::from(version.as_str()).to_string();
    // Use parser spans so unrelated bytes, including mixed line endings and
    // multiline strings, survive without a whole-document render.
    Ok(
        if let Some(span) = document.get("data_version").and_then(toml_edit::Item::span) {
            format!("{}{value}{}", &text[..span.start], &text[span.end..])
        } else {
            let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let (prefix, body) = if let Some(body) = text.strip_prefix('\u{feff}') {
                ("\u{feff}", body)
            } else {
                ("", text)
            };
            format!("{prefix}data_version = {value}{newline}{body}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_migration_registry_agree() {
        for version in SUPPORTED {
            assert_eq!(version.as_str().parse::<DataVersion>().unwrap(), version);
            assert_eq!(
                serde_json::to_string(&version).unwrap(),
                format!("\"{version}\"")
            );
            assert_eq!(
                serde_json::from_str::<DataVersion>(&format!("\"{version}\"")).unwrap(),
                version
            );
        }
        assert!("v1alpha3".parse::<DataVersion>().is_err());
        assert_eq!(MIGRATIONS.len(), SUPPORTED.len() - 1);
        for (step, pair) in MIGRATIONS.iter().zip(SUPPORTED.windows(2)) {
            assert_eq!(step.source, pair[0]);
            assert_eq!(step.target, pair[1]);
        }
    }

    #[test]
    fn migration_replaces_only_the_parsed_value_in_mixed_line_endings() {
        let text = "# Team\r\nformat_version=1\ndata_version  = 'v1alpha1'  # comment\r\n[tags]\nprivate_patterns = [\r\n  'local-*', # keep\n]\r\n";
        assert_eq!(
            migrate(text, LEGACY).unwrap(),
            text.replace("'v1alpha1'", "\"v1alpha2\"")
        );
        assert!(migrate("invalid = [", LEGACY).is_err());
        let original = "\u{feff}# Team\r\nformat_version = 1\r\n";
        let migrated = migrate(original, LEGACY).unwrap();
        assert_eq!(
            migrated,
            "\u{feff}data_version = \"v1alpha2\"\r\n# Team\r\nformat_version = 1\r\n"
        );
        assert_eq!(
            toml::from_str::<toml::Value>(&migrated).unwrap()["data_version"].as_str(),
            Some(CURRENT.as_str())
        );
    }
}
