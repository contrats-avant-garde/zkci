//! Example target for zkci: parses, orders and matches semantic versions.
//!
//! Versions follow [SemVer 2.0.0](https://semver.org) (`1.2.3-rc.1+build.5`)
//! and requirements follow Cargo's syntax (`^1.2`, `~1.2.3`, `>=1.0, <2`,
//! `1.*`). It is pure logic with no dependencies beyond `std`, so the zkVM
//! guest compiles it and its unit tests (`src/tests.rs`) unmodified.

use std::{cmp::Ordering, fmt, str::FromStr};

#[cfg(test)]
mod tests;

/// A version: `MAJOR.MINOR.PATCH[-PRE][+BUILD]`.
///
/// `Ord` orders by SemVer precedence and breaks ties on build metadata, so
/// that it agrees with `Eq`. [`Version::cmp_precedence`] ignores build
/// metadata, as the specification does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers, the dot-separated part after `-`.
    pub pre: Vec<Identifier>,
    /// Build metadata, the dot-separated part after `+`.
    pub build: Vec<String>,
}

/// One pre-release identifier.
///
/// Numeric identifiers compare by value and sort before alphanumeric ones,
/// which compare in ASCII order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identifier {
    Numeric(u64),
    AlphaNumeric(String),
}

/// A requirement: comparators that a version must all satisfy.
///
/// `*` parses to no comparators, which every release satisfies.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VersionReq {
    pub comparators: Vec<Comparator>,
}

/// One comparator, such as `>=1.2.0` or `^0.3`. A missing minor or patch is
/// a wildcard.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Comparator {
    pub op: Op,
    pub major: u64,
    pub minor: Option<u64>,
    pub patch: Option<u64>,
    /// Only set when `patch` is.
    pub pre: Vec<Identifier>,
}

/// A comparator's operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// `=`
    Exact,
    /// `>`
    Greater,
    /// `>=`
    GreaterEq,
    /// `<`
    Less,
    /// `<=`
    LessEq,
    /// `~`: patch updates only, or minor ones if no minor was given.
    Tilde,
    /// `^`, and a bare version: updates that keep the leftmost non-zero part.
    Caret,
    /// `1.*` or `1.2.*`.
    Wildcard,
}

/// Why a string is not a version or a requirement. Positions are byte
/// offsets into the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The input is empty or only whitespace.
    Empty,
    /// The input stops where more was required.
    UnexpectedEnd,
    /// A character that is not allowed at this position.
    UnexpectedChar { pos: usize, found: char },
    /// A numeric part other than `0` that starts with `0`.
    LeadingZero { pos: usize },
    /// A numeric part larger than `u64::MAX`.
    Overflow { pos: usize },
    /// Nothing between two separators in a pre-release or build.
    EmptyIdentifier { pos: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => f.write_str("empty input"),
            Error::UnexpectedEnd => f.write_str("unexpected end of input"),
            Error::UnexpectedChar { pos, found } => {
                write!(f, "unexpected character {found:?} at byte {pos}")
            }
            Error::LeadingZero { pos } => write!(f, "number at byte {pos} has a leading zero"),
            Error::Overflow { pos } => write!(f, "number at byte {pos} does not fit in a u64"),
            Error::EmptyIdentifier { pos } => write!(f, "empty identifier at byte {pos}"),
        }
    }
}

impl std::error::Error for Error {}

impl Version {
    /// A release version with no pre-release or build metadata.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version {
            major,
            minor,
            patch,
            pre: Vec::new(),
            build: Vec::new(),
        }
    }

    /// Parses a version. Surrounding whitespace is not allowed.
    pub fn parse(s: &str) -> Result<Self, Error> {
        if s.is_empty() {
            return Err(Error::Empty);
        }
        let mut c = Cursor::new(s);
        let major = c.number()?;
        c.expect('.')?;
        let minor = c.number()?;
        c.expect('.')?;
        let patch = c.number()?;
        let pre = if c.eat('-') { c.pre()? } else { Vec::new() };
        let build = if c.eat('+') { c.build()? } else { Vec::new() };
        c.finish()?;
        Ok(Version {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }

    /// Whether this is a pre-release.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// Compares by SemVer precedence: build metadata is ignored.
    pub fn cmp_precedence(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }

    /// The next major release: `1.2.3-rc.1` becomes `2.0.0`.
    ///
    /// # Panics
    ///
    /// If the major version is `u64::MAX`.
    pub fn bump_major(&self) -> Self {
        let major = self.major.checked_add(1);
        Version::new(major.expect("major version overflows u64"), 0, 0)
    }

    /// The next minor release: `1.2.3-rc.1` becomes `1.3.0`.
    ///
    /// # Panics
    ///
    /// If the minor version is `u64::MAX`.
    pub fn bump_minor(&self) -> Self {
        let minor = self.minor.checked_add(1);
        Version::new(self.major, minor.expect("minor version overflows u64"), 0)
    }

    /// The next patch release: `1.2.3-rc.1` becomes `1.2.4`.
    ///
    /// # Panics
    ///
    /// If the patch version is `u64::MAX`.
    pub fn bump_patch(&self) -> Self {
        let patch = self.patch.checked_add(1);
        Version::new(
            self.major,
            self.minor,
            patch.expect("patch version overflows u64"),
        )
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_precedence(other)
            .then_with(|| self.build.cmp(&other.build))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Version::parse(s)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", Dotted(&self.pre))?;
        }
        if !self.build.is_empty() {
            write!(f, "+{}", Dotted(&self.build))?;
        }
        Ok(())
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identifier::Numeric(n) => write!(f, "{n}"),
            Identifier::AlphaNumeric(s) => f.write_str(s),
        }
    }
}

/// A release outranks every pre-release of the same version; otherwise
/// identifiers compare left to right and a longer list wins a tie.
fn cmp_pre(a: &[Identifier], b: &[Identifier]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.cmp(b),
    }
}

impl VersionReq {
    /// Parses a comma-separated list of comparators, or `*`.
    pub fn parse(s: &str) -> Result<Self, Error> {
        if s.trim().is_empty() {
            return Err(Error::Empty);
        }
        if s.trim() == "*" {
            return Ok(VersionReq {
                comparators: Vec::new(),
            });
        }
        let mut c = Cursor::new(s);
        let mut comparators = Vec::new();
        loop {
            c.skip_spaces();
            comparators.push(c.comparator()?);
            c.skip_spaces();
            if !c.eat(',') {
                break;
            }
        }
        c.finish()?;
        Ok(VersionReq { comparators })
    }

    /// Whether `version` satisfies every comparator.
    ///
    /// As in Cargo, a pre-release only matches if some comparator names a
    /// pre-release of the same `MAJOR.MINOR.PATCH`: `>=1.2.0-rc.1` admits
    /// `1.2.0-rc.2` but not `1.3.0-alpha`.
    pub fn matches(&self, version: &Version) -> bool {
        self.comparators.iter().all(|c| c.matches(version))
            && (version.pre.is_empty() || self.comparators.iter().any(|c| c.opts_in(version)))
    }

    /// The newest version in `versions` that satisfies the requirement.
    pub fn max_satisfying<'a>(
        &self,
        versions: impl IntoIterator<Item = &'a Version>,
    ) -> Option<&'a Version> {
        versions.into_iter().filter(|v| self.matches(v)).max()
    }
}

impl FromStr for VersionReq {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        VersionReq::parse(s)
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.comparators.is_empty() {
            return f.write_str("*");
        }
        for (i, comparator) in self.comparators.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{comparator}")?;
        }
        Ok(())
    }
}

impl Comparator {
    /// Whether `version` satisfies this comparator on its own, without the
    /// pre-release rule of [`VersionReq::matches`].
    pub fn matches(&self, version: &Version) -> bool {
        match self.op {
            Op::Exact | Op::Wildcard => self.matches_exact(version),
            Op::Greater => self.matches_greater(version),
            Op::GreaterEq => self.matches_exact(version) || self.matches_greater(version),
            Op::Less => self.matches_less(version),
            Op::LessEq => self.matches_exact(version) || self.matches_less(version),
            Op::Tilde => self.matches_tilde(version),
            Op::Caret => self.matches_caret(version),
        }
    }

    fn opts_in(&self, version: &Version) -> bool {
        !self.pre.is_empty()
            && self.major == version.major
            && self.minor == Some(version.minor)
            && self.patch == Some(version.patch)
    }

    fn matches_exact(&self, v: &Version) -> bool {
        v.major == self.major
            && self.minor.is_none_or(|minor| v.minor == minor)
            && self.patch.is_none_or(|patch| v.patch == patch)
            && v.pre == self.pre
    }

    fn matches_greater(&self, v: &Version) -> bool {
        if v.major != self.major {
            return v.major > self.major;
        }
        let Some(minor) = self.minor else {
            return false;
        };
        if v.minor != minor {
            return v.minor > minor;
        }
        let Some(patch) = self.patch else {
            return false;
        };
        if v.patch != patch {
            return v.patch > patch;
        }
        cmp_pre(&v.pre, &self.pre) == Ordering::Greater
    }

    fn matches_less(&self, v: &Version) -> bool {
        if v.major != self.major {
            return v.major < self.major;
        }
        let Some(minor) = self.minor else {
            return false;
        };
        if v.minor != minor {
            return v.minor < minor;
        }
        let Some(patch) = self.patch else {
            return false;
        };
        if v.patch != patch {
            return v.patch < patch;
        }
        cmp_pre(&v.pre, &self.pre) == Ordering::Less
    }

    fn matches_tilde(&self, v: &Version) -> bool {
        if v.major != self.major || self.minor.is_some_and(|minor| v.minor != minor) {
            return false;
        }
        match self.patch {
            Some(patch) if v.patch != patch => v.patch > patch,
            _ => cmp_pre(&v.pre, &self.pre) != Ordering::Less,
        }
    }

    fn matches_caret(&self, v: &Version) -> bool {
        if v.major != self.major {
            return false;
        }
        let Some(minor) = self.minor else {
            return true;
        };
        let Some(patch) = self.patch else {
            return if self.major > 0 {
                v.minor >= minor
            } else {
                v.minor == minor
            };
        };
        if self.major > 0 {
            if v.minor != minor {
                return v.minor > minor;
            }
            if v.patch != patch {
                return v.patch > patch;
            }
        } else if minor > 0 {
            if v.minor != minor {
                return false;
            }
            if v.patch != patch {
                return v.patch > patch;
            }
        } else if v.minor != minor || v.patch != patch {
            return false;
        }
        cmp_pre(&v.pre, &self.pre) != Ordering::Less
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self.op {
            Op::Exact => "=",
            Op::Greater => ">",
            Op::GreaterEq => ">=",
            Op::Less => "<",
            Op::LessEq => "<=",
            Op::Tilde => "~",
            Op::Caret => "^",
            Op::Wildcard => "",
        };
        write!(f, "{op}{}", self.major)?;
        if let Some(minor) = self.minor {
            write!(f, ".{minor}")?;
        }
        if let Some(patch) = self.patch {
            write!(f, ".{patch}")?;
        }
        if !self.pre.is_empty() {
            write!(f, "-{}", Dotted(&self.pre))?;
        }
        if self.op == Op::Wildcard {
            f.write_str(".*")?;
        }
        Ok(())
    }
}

/// Formats a list joined with dots.
struct Dotted<'a, T>(&'a [T]);

impl<T: fmt::Display> fmt::Display for Dotted<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, part) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{part}")?;
        }
        Ok(())
    }
}

/// A position in the input being parsed.
struct Cursor<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Cursor { s, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += c.len_utf8();
        }
        found
    }

    fn unexpected(&self) -> Error {
        match self.peek() {
            Some(found) => Error::UnexpectedChar {
                pos: self.pos,
                found,
            },
            None => Error::UnexpectedEnd,
        }
    }

    fn expect(&mut self, c: char) -> Result<(), Error> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn finish(&self) -> Result<(), Error> {
        match self.peek() {
            None => Ok(()),
            Some(_) => Err(self.unexpected()),
        }
    }

    fn skip_spaces(&mut self) {
        while let Some(c) = self.peek().filter(|c| c.is_whitespace()) {
            self.pos += c.len_utf8();
        }
    }

    /// A run of `[0-9A-Za-z-]`, possibly empty.
    fn word(&mut self) -> &'a str {
        let start = self.pos;
        let len = self.s[start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(self.s.len() - start);
        self.pos += len;
        &self.s[start..start + len]
    }

    /// A decimal number with no leading zero.
    fn number(&mut self) -> Result<u64, Error> {
        let start = self.pos;
        let len = self.s[start..]
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(self.s.len() - start);
        if len == 0 {
            return Err(self.unexpected());
        }
        self.pos += len;
        numeric(&self.s[start..start + len], start)
    }

    /// Dot-separated words, each non-empty.
    fn words(&mut self) -> Result<Vec<(usize, &'a str)>, Error> {
        let mut words = Vec::new();
        loop {
            let pos = self.pos;
            let word = self.word();
            if word.is_empty() {
                return Err(Error::EmptyIdentifier { pos });
            }
            words.push((pos, word));
            if !self.eat('.') {
                return Ok(words);
            }
        }
    }

    fn pre(&mut self) -> Result<Vec<Identifier>, Error> {
        self.words()?
            .into_iter()
            .map(|(pos, word)| {
                if word.bytes().all(|b| b.is_ascii_digit()) {
                    numeric(word, pos).map(Identifier::Numeric)
                } else {
                    Ok(Identifier::AlphaNumeric(word.to_owned()))
                }
            })
            .collect()
    }

    fn build(&mut self) -> Result<Vec<String>, Error> {
        Ok(self
            .words()?
            .into_iter()
            .map(|(_, word)| word.to_owned())
            .collect())
    }

    /// `[op] MAJOR[.MINOR[.PATCH[-PRE]]]`, or a wildcard `MAJOR.*` /
    /// `MAJOR.MINOR.*` with no operator.
    fn comparator(&mut self) -> Result<Comparator, Error> {
        let op = self.op();
        self.skip_spaces();
        let major = self.number()?;
        let mut minor = None;
        let mut patch = None;
        let mut wildcard = false;
        if self.eat('.') {
            if self.wildcard(op)? {
                wildcard = true;
            } else {
                minor = Some(self.number()?);
                if self.eat('.') {
                    if self.wildcard(op)? {
                        wildcard = true;
                    } else {
                        patch = Some(self.number()?);
                    }
                }
            }
        }
        let pre = if patch.is_some() && self.eat('-') {
            self.pre()?
        } else {
            Vec::new()
        };
        let op = match op {
            Some(op) => op,
            None if wildcard => Op::Wildcard,
            None => Op::Caret,
        };
        Ok(Comparator {
            op,
            major,
            minor,
            patch,
            pre,
        })
    }

    fn op(&mut self) -> Option<Op> {
        let ops = [
            (">=", Op::GreaterEq),
            ("<=", Op::LessEq),
            (">", Op::Greater),
            ("<", Op::Less),
            ("=", Op::Exact),
            ("~", Op::Tilde),
            ("^", Op::Caret),
        ];
        let rest = &self.s[self.pos..];
        let (token, op) = ops.into_iter().find(|(t, _)| rest.starts_with(t))?;
        self.pos += token.len();
        Some(op)
    }

    /// Consumes a `*`, which is only allowed without an operator.
    fn wildcard(&mut self, op: Option<Op>) -> Result<bool, Error> {
        if self.peek() != Some('*') {
            return Ok(false);
        }
        if op.is_some() {
            return Err(self.unexpected());
        }
        self.pos += 1;
        Ok(true)
    }
}

/// Parses a run of ASCII digits that started at byte `pos`.
fn numeric(digits: &str, pos: usize) -> Result<u64, Error> {
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(Error::LeadingZero { pos });
    }
    digits.parse().map_err(|_| Error::Overflow { pos })
}
