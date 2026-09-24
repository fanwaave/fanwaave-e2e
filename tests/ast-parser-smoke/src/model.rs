use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable JSON schema identifier for command reports.
pub const REPORT_SCHEMA: &str = "ores.cli.report/v1";

/// Finding severity.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational evidence that does not affect the process exit code.
    Info,
    /// A policy or completeness concern.
    Warning,
    /// A violated required invariant.
    Error,
}

impl Severity {
    /// Whether this severity contributes to the non-zero policy exit code.
    #[must_use]
    pub const fn is_issue(self) -> bool {
        !matches!(self, Self::Info)
    }

    pub(crate) const fn sort_rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        })
    }
}

/// One actionable audit result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable machine-readable code.
    pub code: String,
    /// Severity used for policy exit-code calculation.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Repository, file, organization, or other target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Additional structured evidence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
}

impl Finding {
    /// Create an informational finding.
    #[must_use]
    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, message)
    }

    /// Create a warning finding.
    #[must_use]
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    /// Create an error finding.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            target: None,
            details: BTreeMap::new(),
        }
    }

    /// Return a new finding with a target identifier.
    #[must_use]
    pub fn with_target(self, target: impl Into<String>) -> Self {
        let Self {
            code,
            severity,
            message,
            details,
            ..
        } = self;
        Self {
            code,
            severity,
            message,
            target: Some(target.into()),
            details,
        }
    }

    /// Return a new finding with one additional JSON-serializable detail.
    ///
    /// Findings are cold-path audit values, so this deliberately constructs a
    /// fresh map rather than lending a mutable map to callers. Use
    /// [`Self::with_details`] when adding several details so the rebuild happens
    /// only once.
    #[must_use]
    pub fn with_detail(self, key: impl Into<String>, value: Value) -> Self {
        self.with_details(std::iter::once((key, value)))
    }

    /// Return a new finding with a batch of additional details.
    #[must_use]
    pub fn with_details<I, K>(self, values: I) -> Self
    where
        I: IntoIterator<Item = (K, Value)>,
        K: Into<String>,
    {
        let Self {
            code,
            severity,
            message,
            target,
            details,
        } = self;
        let details = details
            .into_iter()
            .chain(values.into_iter().map(|(key, value)| (key.into(), value)))
            .collect();
        Self {
            code,
            severity,
            message,
            target,
            details,
        }
    }
}

/// Overall report status.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    /// No warning- or error-level findings.
    Passed,
    /// At least one policy finding requires evaluation.
    StoppedForEvaluation,
}

impl fmt::Display for ReportStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Passed => "passed",
            Self::StoppedForEvaluation => "stopped_for_evaluation",
        })
    }
}

/// Complete result of one command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReport {
    /// Report contract identifier.
    pub schema: String,
    /// Canonical command path.
    pub command: String,
    /// Overall status.
    pub status: ReportStatus,
    /// Stable ordered findings.
    pub findings: Vec<Finding>,
    /// Command-level structured evidence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl CommandReport {
    /// Create an empty passed report.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            schema: REPORT_SCHEMA.to_owned(),
            command: command.into(),
            status: ReportStatus::Passed,
            findings: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    /// Return a new report containing one additional finding.
    #[must_use]
    pub fn with_finding(self, finding: Finding) -> Self {
        self.with_findings(std::iter::once(finding))
    }

    /// Return a new report containing a batch of additional findings.
    #[must_use]
    pub fn with_findings<I>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = Finding>,
    {
        self.findings.extend(values);
        self
    }

    /// Return a new report containing one additional metadata entry.
    #[must_use]
    pub fn with_metadata(self, key: impl Into<String>, value: Value) -> Self {
        self.with_metadata_entries(std::iter::once((key, value)))
    }

    /// Return a new report containing a batch of additional metadata entries.
    #[must_use]
    pub fn with_metadata_entries<I, K>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = (K, Value)>,
        K: Into<String>,
    {
        self.metadata
            .extend(values.into_iter().map(|(key, value)| (key.into(), value)));
        self
    }

    /// Add a finding to an incrementally assembled report.
    ///
    /// Prefer [`Self::with_finding`] / [`Self::with_findings`] for ordinary
    /// cold-path transformations. This imperative compatibility API remains for
    /// adapters that naturally accumulate streaming or callback-produced
    /// evidence (resource-heavy or incremental audits) and already hold
    /// exclusive ownership of the report.
    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// Add metadata to an incrementally assembled report.
    ///
    /// Prefer [`Self::with_metadata`] / [`Self::with_metadata_entries`] for
    /// ordinary value transformations.
    pub fn insert_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Number of warning- and error-level findings.
    #[must_use]
    pub fn issue_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity.is_issue())
            .count()
    }

    /// Stable process exit code: 0 for pass, 2 for policy findings.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if self.issue_count() == 0 { 0 } else { 2 }
    }

    /// Finalize status and deterministic finding order into a new report value.
    #[must_use]
    pub fn finalize(self) -> Self {
        let Self {
            schema,
            command,
            findings,
            metadata,
            ..
        } = self;

        // Sorting is intentionally in-place on this exclusively owned vector:
        // allocating/copying the full finding set solely to order it would add
        // cost without improving aliasing safety. No caller-owned collection is
        // mutated because `finalize` consumes the report.
        let mut findings = findings;
        findings.sort_by(|left, right| {
            left.severity
                .sort_rank()
                .cmp(&right.severity.sort_rank())
                .then_with(|| left.code.cmp(&right.code))
                .then_with(|| left.target.cmp(&right.target))
                .then_with(|| left.message.cmp(&right.message))
        });
        let status = if findings.iter().any(|finding| finding.severity.is_issue()) {
            ReportStatus::StoppedForEvaluation
        } else {
            ReportStatus::Passed
        };

        Self {
            schema,
            command,
            status,
            findings,
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CommandReport, Finding, ReportStatus};

    #[test]
    fn information_does_not_fail_report() {
        let report = CommandReport::new("doctor")
            .with_finding(Finding::info("ready", "ready"))
            .finalize();
        assert_eq!(report.status, ReportStatus::Passed);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn warnings_stop_for_evaluation() {
        let report = CommandReport::new("audit repository")
            .with_finding(Finding::warning("missing", "missing"))
            .finalize();
        assert_eq!(report.status, ReportStatus::StoppedForEvaluation);
        assert_eq!(report.exit_code(), 2);
    }

    #[test]
    fn consuming_builders_leave_the_source_value_unchanged() {
        let source = CommandReport::new("audit repository");
        let built = source
            .clone()
            .with_findings([Finding::info("one", "one"), Finding::info("two", "two")])
            .with_metadata("count", json!(2));

        assert!(source.findings.is_empty());
        assert!(source.metadata.is_empty());
        assert_eq!(built.findings.len(), 2);
        assert_eq!(built.metadata.get("count"), Some(&json!(2)));
    }

    #[test]
    fn value_builders_leave_source_values_unchanged() {
        let base_finding = Finding::info("ready", "ready");
        let decorated = base_finding.clone().with_target("repo").with_details([
            ("nested", json!({"values": [1, 2, 3]})),
            ("enabled", json!(true)),
        ]);
        assert_eq!(base_finding.target, None);
        assert!(base_finding.details.is_empty());
        assert_eq!(decorated.target.as_deref(), Some("repo"));
        assert_eq!(decorated.details["nested"], json!({"values": [1, 2, 3]}));

        let base_report = CommandReport::new("doctor");
        let report = base_report
            .clone()
            .with_finding(decorated)
            .with_metadata("summary", json!({"count": 1}))
            .finalize();
        assert!(base_report.findings.is_empty());
        assert!(base_report.metadata.is_empty());
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.metadata["summary"], json!({"count": 1}));
    }

    #[test]
    fn imperative_compatibility_api_still_supports_incremental_adapters() {
        let mut report = CommandReport::new("streaming adapter");
        report.push(Finding::info("first", "first"));
        report.insert_metadata("count", json!(1));
        let report = report.finalize();
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.metadata["count"], json!(1));
    }
}
