//! Store compatibility, upgrade advice, and ordered configuration migrations.

use std::{fmt, str::FromStr};

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use toml_edit::Document;

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
    /// Explicit migration repairs legacy YAML tag indentation without changing values.
    V1Alpha3,
    /// Tags use optional descriptions instead of required topics.
    V1Alpha4,
}

/// Version used when the shared configuration has no `data_version` field.
pub const LEGACY: DataVersion = DataVersion::V1Alpha1;
/// Version written by new stores and the target of an explicit upgrade.
pub const CURRENT: DataVersion = DataVersion::V1Alpha4;
/// All readable and writable data versions, in migration order.
pub const SUPPORTED: [DataVersion; 4] = [
    LEGACY,
    DataVersion::V1Alpha2,
    DataVersion::V1Alpha3,
    CURRENT,
];

impl DataVersion {
    /// Stable spelling used in TOML, JSON, and notices.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1Alpha1 => "v1alpha1",
            Self::V1Alpha2 => "v1alpha2",
            Self::V1Alpha3 => "v1alpha3",
            Self::V1Alpha4 => "v1alpha4",
        }
    }

    /// Whether an explicit upgrade can move this version to the current format.
    pub fn upgrade_available(self) -> bool {
        self != CURRENT
    }

    /// Whether note readers enforce the representation established by v1alpha3.
    pub(crate) const fn requires_valid_tag_indentation(self) -> bool {
        match self {
            Self::V1Alpha1 | Self::V1Alpha2 => false,
            Self::V1Alpha3 | Self::V1Alpha4 => true,
        }
    }

    pub(crate) const fn optional_tag_descriptions(self) -> bool {
        matches!(self, Self::V1Alpha4)
    }

    /// Whether ordinary operations can safely read and preserve this representation.
    /// All registered versions remain usable. Future upgrade-only versions must
    /// return false here and supply a fail-closed migration before registration.
    pub const fn usable(self) -> bool {
        match self {
            Self::V1Alpha1 | Self::V1Alpha2 | Self::V1Alpha3 | Self::V1Alpha4 => true,
        }
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
                        "Unsupported data_version {value:?}; use compatible newer tooling for newer formats; never downgrade. Supported versions: {}",
                        SUPPORTED.map(Self::as_str).join(", ")
                    ),
                )
            })
    }
}

/// Strength of the recommendation, independent of a local deferral preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeRequirement {
    None,
    Should,
    Must,
}

/// Structured guidance for explicit migrations. Public advice never uses private
/// preferences; deferral only silences optional local reminders for one target.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UpgradeAdvice {
    pub source_version: DataVersion,
    pub target_version: DataVersion,
    pub requirement: UpgradeRequirement,
    pub can_upgrade: bool,
    pub deferred: bool,
    pub message: String,
}

impl UpgradeAdvice {
    pub(crate) fn uninitialized() -> Self {
        Self {
            source_version: LEGACY,
            target_version: CURRENT,
            requirement: UpgradeRequirement::None,
            can_upgrade: false,
            deferred: false,
            message: "Store is not initialized; initialize it before upgrading.".into(),
        }
    }

    pub(crate) fn new(source: DataVersion, deferred_target: Option<&str>) -> Self {
        Self::for_support(source, source.usable(), deferred_target)
    }

    fn for_support(source: DataVersion, usable: bool, deferred_target: Option<&str>) -> Self {
        let can_upgrade = source.upgrade_available();
        let requirement = if !usable {
            UpgradeRequirement::Must
        } else if can_upgrade {
            UpgradeRequirement::Should
        } else {
            UpgradeRequirement::None
        };
        let deferred =
            requirement == UpgradeRequirement::Should && deferred_target == Some(CURRENT.as_str());
        let message = match requirement {
            UpgradeRequirement::None => "Store data is current; no upgrade is needed.".into(),
            UpgradeRequirement::Should => format!(
                "Store data version {source} is safely usable. You SHOULD upgrade to {CURRENT}; you MAY invoke upgrade_store or robostickies upgrade."
            ),
            UpgradeRequirement::Must => format!(
                "Store data version {source} cannot be used safely. You MUST upgrade to {CURRENT} with upgrade_store or robostickies upgrade before ordinary operations."
            ),
        };
        Self {
            source_version: source,
            target_version: CURRENT,
            requirement,
            can_upgrade,
            deferred,
            message,
        }
    }

    pub(crate) fn ensure_usable(&self) -> Result<()> {
        if self.requirement == UpgradeRequirement::Must {
            return Err(Error::new("upgrade_required", &self.message).with_upgrade(self.clone()));
        }
        Ok(())
    }

    pub(crate) fn check_access(&self, inspection: bool) -> Result<()> {
        if inspection {
            Ok(())
        } else {
            self.ensure_usable()
        }
    }

    pub(crate) fn reminder(&self) -> bool {
        self.requirement == UpgradeRequirement::Should && !self.deferred
    }
}

/// Location and final retention of an upgrade snapshot, relative to the repository.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SnapshotResult {
    pub path: String,
    pub retained: bool,
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
    /// True when this call recorded a declined optional target.
    pub deferred: bool,
    pub upgrade: UpgradeAdvice,
    /// Absent for previews, deferrals, and already-current stores.
    pub snapshot: Option<SnapshotResult>,
    /// Local preference or snapshot-cleanup warnings; migration itself succeeded.
    pub warnings: Vec<String>,
}

pub(crate) struct Migration {
    source: &'static str,
    target: &'static str,
    transform: fn(&str, &str) -> Result<String>,
}

// Source transformations run at their registered target in Store's migration
// preparation. No initialization, ordinary edit, or cache recovery runs them.
const MIGRATIONS: &[Migration] = &[
    Migration {
        source: LEGACY.as_str(),
        target: DataVersion::V1Alpha2.as_str(),
        transform: migrate_marker,
    },
    Migration {
        source: DataVersion::V1Alpha2.as_str(),
        target: DataVersion::V1Alpha3.as_str(),
        transform: migrate_marker,
    },
    Migration {
        source: DataVersion::V1Alpha3.as_str(),
        target: CURRENT.as_str(),
        transform: migrate_marker,
    },
];

#[cfg(test)]
fn migrate(text: &str, source: DataVersion) -> Result<String> {
    run_migrations(
        text,
        source.as_str(),
        CURRENT.as_str(),
        MIGRATIONS,
        |_, _| Ok(()),
    )
}

/// Resolve the entire path before executing it. Deferrals are deliberately absent:
/// declining one target must never skip a required intermediate transformation.
fn migration_path<'a>(
    source: &str,
    target: &str,
    steps: &'a [Migration],
) -> Result<Vec<&'a Migration>> {
    let mut path = Vec::new();
    let mut version = source;
    while version != target {
        if path.len() >= steps.len() {
            return Err(Error::new(
                "configuration",
                "Incomplete or cyclic migration path",
            ));
        }
        let mut candidates = steps.iter().filter(|step| step.source == version);
        let step = candidates.next().ok_or_else(|| {
            Error::new(
                "configuration",
                "No supported migration path; use compatible tooling",
            )
        })?;
        if candidates.next().is_some() {
            return Err(Error::new("configuration", "Ambiguous migration path"));
        }
        version = step.target;
        path.push(step);
    }
    Ok(path)
}

pub(crate) fn apply_migrations(
    text: &str,
    source: DataVersion,
    validate_step: impl FnMut(&str, &str) -> Result<()>,
) -> Result<String> {
    run_migrations(
        text,
        source.as_str(),
        CURRENT.as_str(),
        MIGRATIONS,
        validate_step,
    )
}

fn run_migrations(
    text: &str,
    source: &str,
    target: &str,
    steps: &[Migration],
    mut validate_step: impl FnMut(&str, &str) -> Result<()>,
) -> Result<String> {
    let mut text = text.to_owned();
    for step in migration_path(source, target, steps)? {
        text = (step.transform)(&text, step.target)?;
        validate_step(&text, step.target)?;
    }
    Ok(text)
}

fn migrate_marker(text: &str, target: &str) -> Result<String> {
    let document = Document::parse(text)
        .map_err(|e| Error::new("configuration", format!("Invalid store config.toml: {e}")))?;
    let value = toml_edit::Value::from(target).to_string();
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
        assert!("v1alpha5".parse::<DataVersion>().is_err());
        assert_eq!(MIGRATIONS.len(), SUPPORTED.len() - 1);
        for (step, pair) in MIGRATIONS.iter().zip(SUPPORTED.windows(2)) {
            assert_eq!(step.source, pair[0].as_str());
            assert_eq!(step.target, pair[1].as_str());
        }
    }

    #[test]
    fn migration_replaces_only_the_parsed_value_in_mixed_line_endings() {
        let text = "# Team\r\nformat_version=1\ndata_version  = 'v1alpha1'  # comment\r\n[tags]\nprivate_patterns = [\r\n  'local-*', # keep\n]\r\n";
        assert_eq!(
            migrate(text, LEGACY).unwrap(),
            text.replace("'v1alpha1'", "\"v1alpha4\"")
        );
        assert!(migrate("invalid = [", LEGACY).is_err());
        let original = "\u{feff}# Team\r\nformat_version = 1\r\n";
        let migrated = migrate(original, LEGACY).unwrap();
        assert_eq!(
            migrated,
            "\u{feff}data_version = \"v1alpha4\"\r\n# Team\r\nformat_version = 1\r\n"
        );
        assert_eq!(
            toml::from_str::<toml::Value>(&migrated).unwrap()["data_version"].as_str(),
            Some(CURRENT.as_str())
        );
    }

    #[test]
    fn toml_1_1_configuration_survives_version_marker_migration() {
        for newline in ["\n", "\r\n"] {
            let text = concat!(
                "# Team configuration\n",
                "format_version = 1\n",
                "data_version = 'v1alpha1' # retain\n",
                "tags = {\n",
                "  default_private = true,\n",
                "  private_patterns = ['local-*'], # retain pattern\n",
                "}\n",
            )
            .replace('\n', newline);
            let migrated = migrate(&text, LEGACY).unwrap();
            assert_eq!(migrated, text.replace("'v1alpha1'", "\"v1alpha4\""));
            crate::config::validate_migration(&migrated, CURRENT.as_str()).unwrap();
        }
    }

    #[test]
    fn deferral_matches_only_the_target_and_cannot_weaken_a_safety_boundary() {
        let normal = UpgradeAdvice::new(LEGACY, None);
        assert!(normal.reminder());
        assert!(normal.message.contains("SHOULD") && normal.message.contains("MAY"));
        assert!(UpgradeAdvice::new(LEGACY, Some("older-target")).reminder());
        let deferred = UpgradeAdvice::new(LEGACY, Some(CURRENT.as_str()));
        assert!(deferred.deferred && !deferred.reminder());
        assert_eq!(deferred.requirement, UpgradeRequirement::Should);
        let mandatory = UpgradeAdvice::for_support(LEGACY, false, Some(CURRENT.as_str()));
        assert!(!mandatory.deferred);
        assert!(mandatory.check_access(true).is_ok());
        assert_eq!(
            mandatory.check_access(false).unwrap_err().code,
            "upgrade_required"
        );
        let error = mandatory.ensure_usable().unwrap_err();
        assert_eq!(error.code, "upgrade_required");
        assert_eq!(error.exit_code(), 78);
        assert_eq!(error.upgrade.unwrap().requirement, UpgradeRequirement::Must);
        assert!(UpgradeAdvice::new(CURRENT, None).ensure_usable().is_ok());
        assert!(!UpgradeAdvice::new(CURRENT, Some(CURRENT.as_str())).deferred);
    }

    #[test]
    fn ordered_steps_validate_each_target_and_stop_at_the_first_failure() {
        // Synthetic versions exercise ordering without registering a new data format.
        let steps = [
            Migration {
                source: "one",
                target: "two",
                transform: migrate_marker,
            },
            Migration {
                source: "two",
                target: "three",
                transform: migrate_marker,
            },
        ];
        let mut seen = vec![];
        let output = run_migrations(
            "# retained\ndata_version = 'one'\n",
            "one",
            "three",
            &steps,
            |text, target| {
                let parsed: toml::Value = toml::from_str(text).unwrap();
                assert_eq!(parsed["data_version"].as_str(), Some(target));
                seen.push(target.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, ["two", "three"]);
        assert!(output.starts_with("# retained\n"));
        seen.clear();
        assert!(
            run_migrations("", "one", "three", &steps, |_, target| {
                seen.push(target.to_owned());
                Err(Error::new("invalid_document", "step validation failed"))
            })
            .is_err()
        );
        assert_eq!(seen, ["two"]);
        assert!(
            run_migrations("invalid = [", "one", "three", &steps, |_, _| panic!(
                "must not save invalid output"
            ))
            .is_err()
        );
        for (source, target) in [("missing", "three"), ("one", "missing")] {
            assert!(
                run_migrations("", source, target, &steps, |_, _| panic!(
                    "resolve the entire path before writes"
                ))
                .is_err()
            );
        }
        let ambiguous = [
            Migration {
                source: "one",
                target: "two",
                transform: migrate_marker,
            },
            Migration {
                source: "one",
                target: "three",
                transform: migrate_marker,
            },
        ];
        assert!(migration_path("one", "three", &ambiguous).is_err());
        let cyclic = [Migration {
            source: "one",
            target: "one",
            transform: migrate_marker,
        }];
        assert!(migration_path("one", "two", &cyclic).is_err());
    }
}
