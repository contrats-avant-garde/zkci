//! Unit tests for parsing, ordering and matching.
//!
//! The zkci guest compiles this file unmodified, so the tests use only the
//! public API and stay deterministic: no I/O, clock, threads or randomness.
//! Their cost ranges from parsing one string to sorting a few hundred
//! versions.

use crate::*;

use std::cmp::Ordering;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

fn req(s: &str) -> VersionReq {
    VersionReq::parse(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

/// Checks `requirement` against each `(version, expected)` pair.
fn check_matches(requirement: &str, cases: &[(&str, bool)]) {
    let parsed = req(requirement);
    for &(version, expected) in cases {
        assert_eq!(
            parsed.matches(&v(version)),
            expected,
            "{requirement} vs {version}"
        );
    }
}

/// Every `MAJOR.MINOR.PATCH` with parts below `bound`, each preceded by four
/// pre-releases, in ascending order. Built from strings, so the parser runs
/// on every one.
fn generated(bound: u64) -> Vec<Version> {
    let mut versions = Vec::new();
    for major in 0..bound {
        for minor in 0..bound {
            for patch in 0..bound {
                for pre in ["-alpha", "-alpha.1", "-beta", "-rc.2", ""] {
                    versions.push(v(&format!("{major}.{minor}.{patch}{pre}")));
                }
            }
        }
    }
    versions
}

#[test]
fn parses_a_release() {
    let version = v("1.2.3");
    assert_eq!(version, Version::new(1, 2, 3));
    assert!(!version.is_prerelease());
    assert!(version.build.is_empty());
}

#[test]
fn parses_prerelease_and_build() -> Result<(), Error> {
    let version: Version = "1.0.0-alpha.1.x-y+build.007.sha-5114f85".parse()?;
    assert_eq!((version.major, version.minor, version.patch), (1, 0, 0));
    assert_eq!(
        version.pre,
        [
            Identifier::AlphaNumeric("alpha".into()),
            Identifier::Numeric(1),
            Identifier::AlphaNumeric("x-y".into()),
        ]
    );
    assert_eq!(version.build, ["build", "007", "sha-5114f85"]);
    assert!(version.is_prerelease());
    Ok(())
}

#[test]
fn rejects_malformed_versions() {
    let cases = [
        ("", Error::Empty),
        ("1", Error::UnexpectedEnd),
        ("1.2", Error::UnexpectedEnd),
        ("1.2.", Error::UnexpectedEnd),
        ("v1.2.3", Error::UnexpectedChar { pos: 0, found: 'v' }),
        (" 1.2.3", Error::UnexpectedChar { pos: 0, found: ' ' }),
        ("1.2.3 ", Error::UnexpectedChar { pos: 5, found: ' ' }),
        ("1.2.3.4", Error::UnexpectedChar { pos: 5, found: '.' }),
        ("1.-2.3", Error::UnexpectedChar { pos: 2, found: '-' }),
        ("1.2.3-b@ta", Error::UnexpectedChar { pos: 7, found: '@' }),
        ("1.2.3-", Error::EmptyIdentifier { pos: 6 }),
        ("1.2.3-alpha..1", Error::EmptyIdentifier { pos: 12 }),
        ("1.2.3+", Error::EmptyIdentifier { pos: 6 }),
    ];
    for (input, error) in cases {
        assert_eq!(Version::parse(input), Err(error), "{input:?}");
    }
    assert_eq!(
        Version::parse("v1.2.3").unwrap_err().to_string(),
        "unexpected character 'v' at byte 0"
    );
}

#[test]
fn rejects_leading_zeros_in_numbers() {
    assert_eq!(Version::parse("01.2.3"), Err(Error::LeadingZero { pos: 0 }));
    assert_eq!(Version::parse("1.02.3"), Err(Error::LeadingZero { pos: 2 }));
    assert_eq!(
        Version::parse("1.2.3-rc.01"),
        Err(Error::LeadingZero { pos: 9 })
    );
    // Zero itself is fine, and so are leading zeros in alphanumeric
    // identifiers and in build metadata.
    assert_eq!(v("0.0.0"), Version::new(0, 0, 0));
    assert_eq!(
        v("1.2.3-0a.0").pre,
        [
            Identifier::AlphaNumeric("0a".into()),
            Identifier::Numeric(0)
        ]
    );
    assert_eq!(v("1.2.3+001").build, ["001"]);
}

#[test]
fn rejects_numbers_beyond_u64() {
    let max = u64::MAX;
    assert_eq!(
        v(&format!("{max}.{max}.{max}")),
        Version::new(max, max, max)
    );
    assert_eq!(
        Version::parse("18446744073709551616.0.0"),
        Err(Error::Overflow { pos: 0 })
    );
    assert_eq!(
        Version::parse("1.0.0-99999999999999999999"),
        Err(Error::Overflow { pos: 6 })
    );
}

#[test]
fn display_round_trips() {
    for s in [
        "0.0.0",
        "10.20.30",
        "1.0.0-alpha",
        "1.0.0-0.3.7",
        "1.0.0-x.7.z.92",
        "1.0.0-x-y-z.--",
        "1.0.0+20130313144700",
        "1.0.0-beta+exp.sha.5114f85",
        "1.0.0+21AF26D3----117B344092BD",
    ] {
        assert_eq!(v(s).to_string(), s);
    }
}

#[test]
fn compares_numerically_not_lexically() {
    assert!(v("1.10.0") > v("1.9.0"));
    assert!(v("2.0.0") > v("1.99.99"));
    assert!(v("1.0.10") > v("1.0.9"));
    assert!(v("1.0.0-rc.10") > v("1.0.0-rc.9"));
    assert_eq!(v("1.2.3").cmp(&v("1.2.3")), Ordering::Equal);
}

#[test]
fn follows_the_spec_precedence_chain() {
    // SemVer 2.0.0, section 11, extended past the release.
    let chain = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.1.0",
        "2.0.0",
    ]
    .map(v);
    for (i, a) in chain.iter().enumerate() {
        for (j, b) in chain.iter().enumerate() {
            assert_eq!(a.cmp_precedence(b), i.cmp(&j), "{a} vs {b}");
        }
    }
}

#[test]
fn build_metadata_does_not_affect_precedence() {
    let a = v("1.0.0+build.1");
    let b = v("1.0.0+build.2");
    assert_eq!(a.cmp_precedence(&b), Ordering::Equal);
    assert_eq!(a.cmp_precedence(&v("1.0.0")), Ordering::Equal);
    assert_ne!(a, b);
    // Ord breaks the tie, so that it agrees with Eq.
    assert!(a < b);
}

#[test]
fn sorts_a_shuffled_set() {
    let ascending = generated(4);
    assert!(ascending.windows(2).all(|w| w[0] < w[1]));
    let n = ascending.len();
    // A fixed permutation: 97 is coprime to n, so i * 97 % n visits every
    // index once.
    let mut shuffled: Vec<Version> = (0..n).map(|i| ascending[i * 97 % n].clone()).collect();
    assert_ne!(shuffled, ascending);
    shuffled.sort();
    assert_eq!(shuffled, ascending);
}

#[test]
fn caret_requirements() {
    check_matches(
        "^1.2.3",
        &[
            ("1.2.3", true),
            ("1.9.0", true),
            ("1.2.2", false),
            ("2.0.0", false),
        ],
    );
    check_matches(
        "^0.2.3",
        &[("0.2.3", true), ("0.2.9", true), ("0.3.0", false)],
    );
    check_matches("^0.0.3", &[("0.0.3", true), ("0.0.4", false)]);
    check_matches(
        "^1.2",
        &[
            ("1.2.0", true),
            ("1.5.1", true),
            ("1.1.9", false),
            ("2.0.0", false),
        ],
    );
    check_matches("^0.0", &[("0.0.7", true), ("0.1.0", false)]);
    check_matches("^1", &[("1.0.0", true), ("1.99.0", true), ("2.0.0", false)]);
    // A bare version is a caret requirement.
    check_matches("1.2.3", &[("1.4.0", true), ("2.0.0", false)]);
}

#[test]
fn tilde_requirements() {
    check_matches(
        "~1.2.3",
        &[
            ("1.2.3", true),
            ("1.2.9", true),
            ("1.2.2", false),
            ("1.3.0", false),
        ],
    );
    check_matches(
        "~1.2",
        &[("1.2.0", true), ("1.2.7", true), ("1.3.0", false)],
    );
    check_matches(
        "~1",
        &[
            ("1.0.0", true),
            ("1.9.9", true),
            ("0.9.0", false),
            ("2.0.0", false),
        ],
    );
}

#[test]
fn wildcard_and_exact_requirements() {
    check_matches(
        "*",
        &[("0.0.0", true), ("9.9.9", true), ("1.0.0-rc.1", false)],
    );
    check_matches(
        "1.*",
        &[
            ("1.0.0", true),
            ("1.7.3", true),
            ("0.9.0", false),
            ("2.0.0", false),
        ],
    );
    check_matches(
        "1.2.*",
        &[("1.2.0", true), ("1.2.9", true), ("1.3.0", false)],
    );
    check_matches(
        "=1.2.3",
        &[("1.2.3", true), ("1.2.3+build", true), ("1.2.4", false)],
    );
    check_matches(
        "=1.2",
        &[("1.2.0", true), ("1.2.9", true), ("1.3.0", false)],
    );
}

#[test]
fn comparison_operators() {
    check_matches(">1.2.3", &[("1.2.4", true), ("1.2.3", false)]);
    check_matches(">1.2", &[("1.3.0", true), ("1.2.9", false)]);
    check_matches(">=1.2", &[("1.2.0", true), ("1.1.9", false)]);
    check_matches("<1.2.3", &[("1.2.2", true), ("1.2.3", false)]);
    check_matches("<1.2", &[("1.1.9", true), ("1.2.0", false)]);
    check_matches("<=1.2", &[("1.2.9", true), ("1.3.0", false)]);
    check_matches("<= 2", &[("2.9.9", true), ("3.0.0", false)]);
}

#[test]
fn requirement_lists_are_intersections() {
    check_matches(
        ">=1.2, <1.5",
        &[
            ("1.2.0", true),
            ("1.4.9", true),
            ("1.1.0", false),
            ("1.5.0", false),
        ],
    );
    check_matches(
        "^1.2, ~1.4",
        &[("1.4.2", true), ("1.3.0", false), ("1.5.0", false)],
    );
    assert_eq!(req(" >=1.2 ,<1.5 ").comparators.len(), 2);
}

#[test]
fn prereleases_match_only_when_named() {
    check_matches("^1.2.3", &[("1.2.3-alpha", false), ("1.3.0-alpha", false)]);
    check_matches(
        ">=1.2.3-alpha.2",
        &[
            ("1.2.3-alpha.10", true),
            ("1.2.3-beta", true),
            ("1.2.3", true),
            ("1.2.4", true),
            ("1.2.3-alpha.1", false),
            ("1.2.4-alpha", false),
        ],
    );
    check_matches(
        "~1.2.3-rc.1",
        &[("1.2.3-rc.2", true), ("1.2.5", true), ("1.2.5-rc.3", false)],
    );
}

#[test]
fn max_satisfying_picks_the_newest_match() {
    let published = [
        "0.9.0",
        "1.0.0",
        "1.0.1",
        "1.1.0-rc.1",
        "1.1.0",
        "1.2.0",
        "1.2.1",
        "1.10.0",
        "2.0.0-alpha",
        "2.0.0",
        "2.1.3",
    ]
    .map(v);
    let newest = |r: &str| req(r).max_satisfying(&published).map(ToString::to_string);
    assert_eq!(newest("*").as_deref(), Some("2.1.3"));
    assert_eq!(newest("^1.0").as_deref(), Some("1.10.0"));
    assert_eq!(newest("~1.2").as_deref(), Some("1.2.1"));
    assert_eq!(newest("<1.1").as_deref(), Some("1.0.1"));
    assert_eq!(newest("=1.1.0-rc.1").as_deref(), Some("1.1.0-rc.1"));
    assert_eq!(
        newest(">=2.0.0-alpha, <2.0.0").as_deref(),
        Some("2.0.0-alpha")
    );
    assert_eq!(newest("^3"), None);
}

#[test]
fn requirement_display_round_trips() {
    for s in [
        "*",
        "^1.2.3",
        "=1.2.3-rc.1",
        ">=1.2, <1.5",
        "~0.3",
        "<=2",
        "1.*",
        "1.2.*",
    ] {
        assert_eq!(req(s).to_string(), s);
    }
    assert_eq!(req("1.2.3").to_string(), "^1.2.3");
    assert_eq!(req(">= 1.2 ,<1.5").to_string(), ">=1.2, <1.5");
}

#[test]
fn rejects_malformed_requirements() {
    let cases = [
        ("", Error::Empty),
        ("  ", Error::Empty),
        (">=", Error::UnexpectedEnd),
        ("^1.2.3,", Error::UnexpectedEnd),
        ("1.2.3 4", Error::UnexpectedChar { pos: 6, found: '4' }),
        ("!=1.2.3", Error::UnexpectedChar { pos: 0, found: '!' }),
        (">=1.*", Error::UnexpectedChar { pos: 4, found: '*' }),
        ("1.*.3", Error::UnexpectedChar { pos: 3, found: '.' }),
        ("^1.2-rc.1", Error::UnexpectedChar { pos: 4, found: '-' }),
        ("^01.2", Error::LeadingZero { pos: 1 }),
    ];
    for (input, error) in cases {
        assert_eq!(VersionReq::parse(input), Err(error), "{input:?}");
    }
}

#[test]
fn bumps_reset_the_lower_parts() {
    let version = v("1.2.3-rc.1+build.5");
    assert_eq!(version.bump_major(), Version::new(2, 0, 0));
    assert_eq!(version.bump_minor(), Version::new(1, 3, 0));
    assert_eq!(version.bump_patch(), Version::new(1, 2, 4));
    assert!(version.bump_patch() > version);
}

#[test]
#[should_panic(expected = "major version overflows u64")]
fn bump_major_past_u64_max_panics() {
    Version::new(u64::MAX, 0, 0).bump_major();
}

#[test]
fn comparators_agree_with_precedence() {
    // Without the pre-release rule, `>=P`, `<P` and `=P` split every
    // version exactly where precedence does.
    let versions = generated(3);
    for pivot in ["0.0.0-alpha", "0.2.1", "1.1.1-beta", "1.2.0-rc.2", "2.0.0"] {
        let p = v(pivot);
        let [ge, lt, eq] =
            [">=", "<", "="].map(|op| req(&format!("{op}{pivot}")).comparators.remove(0));
        for version in &versions {
            let order = version.cmp_precedence(&p);
            assert_eq!(ge.matches(version), order.is_ge(), "{version} >= {pivot}");
            assert_eq!(lt.matches(version), order.is_lt(), "{version} < {pivot}");
            assert_eq!(eq.matches(version), order.is_eq(), "{version} = {pivot}");
        }
    }
}
