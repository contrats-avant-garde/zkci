//! Checks on what a verified proof claims.
//!
//! These run only after the evidence has verified, so they trust that the
//! report is what the guest committed. They decide whether that report is
//! the one the merge gate asked for: the right tree, the right report
//! layout, and every test passing.

use serde::Serialize;
use zkci_harness::{ExcludedTest, SuiteReport, REPORT_VERSION};

/// The verifier's JSON output.
#[derive(Debug, Default, Serialize)]
pub struct Verdict {
    /// True when the evidence verified and [`Self::problems`] is empty.
    pub ok: bool,
    /// `evidence.type` from the envelope.
    pub evidence: String,
    /// The verifying-key hash the proof was checked against.
    pub vk_hash: String,
    /// From the verified report, when there is one.
    pub suite: Option<String>,
    /// From the verified report, when there is one.
    pub tree: Option<String>,
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed.
    pub failed: usize,
    /// Tests the guest could not run, reported but not failing the check.
    pub excluded: Vec<ExcludedTest>,
    /// Everything that makes the verdict negative.
    pub problems: Vec<String>,
}

/// Decodes a guest's public values (`sp1_zkvm::io::commit` writes bincode).
/// Trailing bytes are an error: the guest commits exactly one report.
pub fn decode_report(public_values: &[u8]) -> anyhow::Result<SuiteReport> {
    let report: SuiteReport = bincode::deserialize(public_values)
        .map_err(|e| anyhow::anyhow!("public values are not a SuiteReport: {e}"))?;
    let used = bincode::serialized_size(&report)?;
    if used != public_values.len() as u64 {
        anyhow::bail!(
            "public values carry {} bytes after the report",
            public_values.len() as u64 - used
        );
    }
    Ok(report)
}

/// Compares a verified report with what the caller expects.
pub fn check_report(report: &SuiteReport, expect_tree: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if report.version != REPORT_VERSION {
        problems.push(format!(
            "report version {} (this verifier reads {REPORT_VERSION})",
            report.version
        ));
    }
    if normalize_hex(&report.tree) != normalize_hex(expect_tree) {
        problems.push(format!(
            "proof is for tree {}, expected {expect_tree}",
            report.tree
        ));
    }
    if report.results.is_empty() {
        problems.push("no tests ran".into());
    }
    problems.extend(
        report
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| format!("{} failed", r.name)),
    );
    problems
}

/// Lowercase, without a `0x` prefix, for comparing hashes.
pub fn normalize_hex(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
        .to_ascii_lowercase()
}

impl Verdict {
    /// Fills the report fields and folds in [`check_report`].
    pub fn with_report(mut self, report: &SuiteReport, expect_tree: &str) -> Self {
        self.problems.extend(check_report(report, expect_tree));
        self.suite = Some(report.suite.clone());
        self.tree = Some(report.tree.clone());
        self.passed = report.passed();
        self.failed = report.failed();
        self.excluded = report.excluded.clone();
        self.ok = self.problems.is_empty();
        self
    }
}

#[cfg(test)]
mod tests {
    use zkci_harness::TestResult;

    use super::*;

    fn report(tree: &str, results: &[(&str, bool)]) -> SuiteReport {
        SuiteReport {
            version: REPORT_VERSION,
            suite: "zkci-example".into(),
            tree: tree.into(),
            results: results
                .iter()
                .map(|(name, passed)| TestResult {
                    name: (*name).into(),
                    passed: *passed,
                })
                .collect(),
            excluded: vec![],
        }
    }

    #[test]
    fn passing_report_for_the_expected_tree_is_ok() {
        let r = report("ABCDEF", &[("tests::a", true), ("tests::b", true)]);
        let verdict = Verdict::default().with_report(&r, "0xabcdef");
        assert!(verdict.ok, "{:?}", verdict.problems);
        assert_eq!((verdict.passed, verdict.failed), (2, 0));
    }

    #[test]
    fn wrong_tree_is_a_problem() {
        let problems = check_report(&report("aaaa", &[("tests::a", true)]), "bbbb");
        assert_eq!(problems, ["proof is for tree aaaa, expected bbbb"]);
    }

    #[test]
    fn failures_and_empty_suites_are_problems() {
        let problems = check_report(&report("t", &[("tests::a", false)]), "t");
        assert_eq!(problems, ["tests::a failed"]);
        let problems = check_report(&report("t", &[]), "t");
        assert_eq!(problems, ["no tests ran"]);
    }

    #[test]
    fn unknown_report_version_is_a_problem() {
        let mut r = report("t", &[("tests::a", true)]);
        r.version = REPORT_VERSION + 1;
        assert_eq!(check_report(&r, "t").len(), 1);
    }

    #[test]
    fn problems_carried_in_make_the_verdict_negative() {
        let verdict = Verdict {
            problems: vec!["vk mismatch".into()],
            ..Verdict::default()
        }
        .with_report(&report("t", &[("tests::a", true)]), "t");
        assert!(!verdict.ok);
    }

    #[test]
    fn decode_report_round_trips_and_rejects_trailing_bytes() {
        let r = report("t", &[("tests::a", true)]);
        let mut bytes = bincode::serialize(&r).unwrap();
        assert_eq!(decode_report(&bytes).unwrap(), r);
        bytes.push(0);
        assert!(decode_report(&bytes).is_err());
        assert!(decode_report(&[1, 2, 3]).is_err());
    }

    #[test]
    fn normalize_hex_strips_prefix_and_case() {
        assert_eq!(normalize_hex(" 0xAbC "), "abc");
        assert_eq!(normalize_hex("0XAB"), "ab");
        assert_eq!(normalize_hex("ab"), "ab");
    }
}
