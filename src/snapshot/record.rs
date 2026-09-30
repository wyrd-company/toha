// ---
// relationships:
//   implements: architecture
// ---
//! The snapshot record: its identifier, its `snapshot.json` model, and the
//! read boundary that refuses a malformed or forged snapshot before any render.
//!
//! Everything here is pure: it turns bytes and a ref identity into a validated
//! [`Snapshot`], or a [`SnapshotError`] that names what failed without ever
//! quoting an answer value or a file's content. The git-level checks a snapshot
//! also needs (parentless commit, tree shape, entry kinds) belong to the
//! repository seam, which calls [`Snapshot::validate`] once it has listed the
//! files under `files/`.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::plan::TargetPath;

/// The document format version this build writes and is the only one it reads.
const SNAPSHOT_FORMAT: u32 = 1;

// ---------------------------------------------------------------------------
// SnapshotId — a ULID
// ---------------------------------------------------------------------------

/// A snapshot identifier: a 128-bit ULID rendered as 26 Crockford base32
/// characters, unique across clones, ordered by creation time, never changed.
///
/// Commands accept any unique prefix of at least six characters
/// ([`SnapshotId::parse_prefix`]); a full id round-trips through [`Display`] and
/// [`FromStr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId(u128);

/// Crockford base32 alphabet: the digits without `I`, `L`, `O`, and `U`.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The rendered length of a ULID.
const ULID_LEN: usize = 26;

/// The shortest accepted prefix.
const MIN_PREFIX: usize = 6;

impl SnapshotId {
    /// Build an id from a 48-bit millisecond timestamp and 80 bits of entropy,
    /// as the ULID specification lays them out. The runtime supplies real time
    /// and randomness; tests supply fixed bytes for a deterministic id.
    pub fn from_parts(timestamp_ms: u64, entropy: [u8; 10]) -> Self {
        let ts = u128::from(timestamp_ms & 0x0000_FFFF_FFFF_FFFF);
        let mut rand = 0u128;
        for byte in entropy {
            rand = (rand << 8) | u128::from(byte);
        }
        Self((ts << 80) | rand)
    }

    /// The canonical prefix that matches at least this id, normalised to
    /// Crockford's canonical characters (uppercase, `I`/`L`→`1`, `O`→`0`), or an
    /// error when the text is not a usable prefix.
    ///
    /// A prefix must be between six and twenty-six characters and hold only
    /// Crockford digits; this rejects the accidental short or mistyped input a
    /// refusal would otherwise blame on the snapshot store.
    pub fn parse_prefix(text: &str) -> Result<String, SnapshotError> {
        let trimmed = text.trim();
        if trimmed.len() < MIN_PREFIX || trimmed.len() > ULID_LEN {
            return Err(SnapshotError::Prefix(trimmed.to_owned()));
        }
        let mut out = String::with_capacity(trimmed.len());
        for ch in trimmed.chars() {
            out.push(char::from(
                canonical_digit(ch).ok_or_else(|| SnapshotError::Prefix(trimmed.to_owned()))?,
            ));
        }
        Ok(out)
    }

    /// Whether the id's rendering begins with an already-normalised `prefix`
    /// from [`SnapshotId::parse_prefix`].
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.to_string().starts_with(prefix)
    }
}

/// Map one input character to its canonical Crockford digit, folding case and
/// the accepted `I`/`L`/`O` aliases, or `None` when it is not a digit.
fn canonical_digit(ch: char) -> Option<u8> {
    let upper = ch.to_ascii_uppercase();
    match upper {
        'I' | 'L' => Some(b'1'),
        'O' => Some(b'0'),
        _ if CROCKFORD.contains(&(upper as u8)) => Some(upper as u8),
        _ => None,
    }
}

/// Map one input character to its 5-bit value.
fn digit_value(ch: char) -> Option<u32> {
    let canon = canonical_digit(ch)?;
    CROCKFORD.iter().position(|&d| d == canon).map(|p| p as u32)
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; ULID_LEN];
        let mut value = self.0;
        for slot in buf.iter_mut().rev() {
            *slot = CROCKFORD[(value & 0x1f) as usize];
            value >>= 5;
        }
        // Safety: every byte is an ASCII character from CROCKFORD.
        f.write_str(std::str::from_utf8(&buf).expect("crockford is ascii"))
    }
}

impl FromStr for SnapshotId {
    type Err = SnapshotError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != ULID_LEN {
            return Err(SnapshotError::Id(text.to_owned()));
        }
        let mut value: u128 = 0;
        for ch in text.chars() {
            let digit = digit_value(ch).ok_or_else(|| SnapshotError::Id(text.to_owned()))?;
            value = (value << 5) | u128::from(digit);
        }
        // Twenty-six base32 digits carry 130 bits; the top two bits must be zero
        // for the value to be a 128-bit ULID.
        let first = digit_value(text.chars().next().expect("length checked")).expect("valid digit");
        if first > 7 {
            return Err(SnapshotError::Id(text.to_owned()));
        }
        Ok(Self(value))
    }
}

impl Serialize for SnapshotId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SnapshotId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Supporting value types
// ---------------------------------------------------------------------------

/// A repository-relative path: the forward-slash form git reports, `.` for the
/// repository root. It is always relative, never enters `.git`, and never
/// escapes the repository with `..`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoPath(String);

impl RepoPath {
    /// The repository root.
    pub fn root() -> Self {
        Self(".".to_owned())
    }

    /// Parse a repository-relative path. `.` and the empty string both name the
    /// root. Any other value must be a clean relative path.
    pub fn parse(text: &str) -> Result<Self, SnapshotError> {
        if text.is_empty() || text == "." {
            return Ok(Self::root());
        }
        // A non-root repo path obeys the same rules a target path does.
        TargetPath::parse(text).map_err(|message| SnapshotError::Path {
            path: text.to_owned(),
            message,
        })?;
        Ok(Self(text.replace('\\', "/")))
    }

    /// Whether this path names the repository root.
    pub fn is_root(&self) -> bool {
        self.0 == "."
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A resolved 40-hex git commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitId(String);

impl CommitId {
    /// Parse a lowercase 40-hex commit id.
    pub fn parse(text: &str) -> Result<Self, SnapshotError> {
        let ok = text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit());
        if ok {
            Ok(Self(text.to_ascii_lowercase()))
        } else {
            Err(SnapshotError::Commit(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The template revision a snapshot was built at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// A git-addressed template resolved to this commit.
    Commit(CommitId),
    /// A folder template, which has no commit.
    Unversioned,
}

/// A repository `HEAD` point recorded with the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPoint {
    commit: CommitId,
    branch: Option<String>,
}

impl ProjectPoint {
    pub fn commit(&self) -> &CommitId {
        &self.commit
    }

    /// The branch name, or `None` on a detached `HEAD`.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }
}

/// The instant every render of a project uses, frozen at the first snapshot so a
/// template that renders a date stays byte-stable across updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenNow(jiff::Zoned);

impl FrozenNow {
    /// Wrap a zoned instant.
    pub fn new(instant: jiff::Zoned) -> Self {
        Self(instant)
    }

    fn parse(text: &str) -> Result<Self, SnapshotError> {
        text.parse::<jiff::Zoned>()
            .map(Self)
            .map_err(|_| SnapshotError::Instant(text.to_owned()))
    }

    pub fn get(&self) -> &jiff::Zoned {
        &self.0
    }
}

impl fmt::Display for FrozenNow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// When a snapshot was saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(jiff::Timestamp);

impl Timestamp {
    fn parse(text: &str) -> Result<Self, SnapshotError> {
        text.parse::<jiff::Timestamp>()
            .map(Self)
            .map_err(|_| SnapshotError::Instant(text.to_owned()))
    }

    pub fn get(&self) -> jiff::Timestamp {
        self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What Toha owns at one captured path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A whole file Toha owns.
    Toha,
    /// A file Toha edits inside, owning the listed region keys and JSON paths.
    Edit {
        regions: Vec<String>,
        values: Vec<String>,
    },
    /// A file a hook created or changed.
    Hook,
}

/// One entry in a snapshot's `paths`: a captured file and what Toha owns there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathOwnership {
    path: TargetPath,
    origin: Origin,
}

impl PathOwnership {
    pub fn path(&self) -> &TargetPath {
        &self.path
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }
}

// ---------------------------------------------------------------------------
// The wire model — snapshot.json exactly, unknown members denied
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocWire {
    snapshot: u32,
    id: String,
    template: String,
    source: String,
    commit: Option<String>,
    target: String,
    created: String,
    generated: String,
    project: ProjectWire,
    built_from: Option<String>,
    submissions: Vec<IndexMap<String, serde_json::Value>>,
    paths: IndexMap<String, PathWire>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectWire {
    commit: String,
    branch: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathWire {
    origin: OriginWire,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    regions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OriginWire {
    Toha,
    Edit,
    Hook,
}

// ---------------------------------------------------------------------------
// The validated snapshot
// ---------------------------------------------------------------------------

/// A snapshot's validated `snapshot.json`, independent of its git ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDoc {
    id: SnapshotId,
    template: String,
    source: String,
    revision: Revision,
    target: RepoPath,
    created: Timestamp,
    generated: FrozenNow,
    project: ProjectPoint,
    built_from: Option<SnapshotId>,
    submissions: Vec<IndexMap<String, serde_json::Value>>,
    paths: Vec<PathOwnership>,
}

/// A validated snapshot: its document plus the id proven equal to its ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    doc: SnapshotDoc,
}

impl Snapshot {
    /// Validate a snapshot at the read boundary.
    ///
    /// `ref_id` is the id parsed from the ref name; `bytes` is the raw
    /// `snapshot.json`; `files` is the exact set of paths under `files/` the
    /// repository seam listed. A snapshot fetched from a teammate is data that
    /// can be refused, never instructions, so every check that fails refuses the
    /// whole snapshot.
    pub fn validate(
        ref_id: SnapshotId,
        bytes: &[u8],
        files: &BTreeSet<String>,
    ) -> Result<Self, SnapshotError> {
        let wire: DocWire = serde_json::from_slice(bytes).map_err(SnapshotError::Json)?;

        if wire.snapshot != SNAPSHOT_FORMAT {
            return Err(SnapshotError::Format(wire.snapshot));
        }

        let id: SnapshotId = wire.id.parse()?;
        if id != ref_id {
            return Err(SnapshotError::IdRefMismatch {
                declared: id,
                r#ref: ref_id,
            });
        }

        if wire.source != strip_reference(&wire.template) {
            return Err(SnapshotError::Source {
                template: wire.template.clone(),
                declared: wire.source.clone(),
            });
        }

        let revision = match wire.commit.as_deref() {
            None => Revision::Unversioned,
            Some(text) => Revision::Commit(CommitId::parse(text)?),
        };

        let target = RepoPath::parse(&wire.target)?;
        let created = Timestamp::parse(&wire.created)?;
        let generated = FrozenNow::parse(&wire.generated)?;
        let project = ProjectPoint {
            commit: CommitId::parse(&wire.project.commit)?,
            branch: wire.project.branch,
        };
        let built_from = match wire.built_from {
            None => None,
            Some(text) => Some(text.parse()?),
        };

        let mut paths = Vec::with_capacity(wire.paths.len());
        let mut keys = BTreeSet::new();
        for (raw, entry) in wire.paths {
            let path = TargetPath::parse(&raw).map_err(|message| SnapshotError::Path {
                path: raw.clone(),
                message,
            })?;
            let origin = validate_origin(&raw, entry)?;
            keys.insert(raw);
            paths.push(PathOwnership { path, origin });
        }

        if &keys != files {
            return Err(SnapshotError::PathsFilesMismatch);
        }

        Ok(Self {
            doc: SnapshotDoc {
                id,
                template: wire.template,
                source: wire.source,
                revision,
                target,
                created,
                generated,
                project,
                built_from,
                submissions: wire.submissions,
                paths,
            },
        })
    }

    pub fn id(&self) -> &SnapshotId {
        &self.doc.id
    }

    pub fn template(&self) -> &str {
        &self.doc.template
    }

    pub fn source(&self) -> &str {
        &self.doc.source
    }

    pub fn revision(&self) -> &Revision {
        &self.doc.revision
    }

    pub fn target(&self) -> &RepoPath {
        &self.doc.target
    }

    pub fn created(&self) -> Timestamp {
        self.doc.created
    }

    pub fn generated(&self) -> &FrozenNow {
        &self.doc.generated
    }

    pub fn project(&self) -> &ProjectPoint {
        &self.doc.project
    }

    pub fn built_from(&self) -> Option<&SnapshotId> {
        self.doc.built_from.as_ref()
    }

    pub fn submissions(&self) -> &[IndexMap<String, serde_json::Value>] {
        &self.doc.submissions
    }

    /// Every captured path and what Toha owns there.
    pub fn paths(&self) -> &[PathOwnership] {
        &self.doc.paths
    }
}

/// The source identity of a formal name: everything before its first `@`.
fn strip_reference(template: &str) -> &str {
    template
        .split_once('@')
        .map_or(template, |(source, _)| source)
}

/// Turn a wire path entry into a validated [`Origin`], refusing region or value
/// lists on an origin that cannot own them.
fn validate_origin(path: &str, entry: PathWire) -> Result<Origin, SnapshotError> {
    match entry.origin {
        OriginWire::Toha | OriginWire::Hook
            if !entry.regions.is_empty() || !entry.values.is_empty() =>
        {
            Err(SnapshotError::Ownership {
                path: path.to_owned(),
            })
        }
        OriginWire::Toha => Ok(Origin::Toha),
        OriginWire::Hook => Ok(Origin::Hook),
        OriginWire::Edit => Ok(Origin::Edit {
            regions: entry.regions,
            values: entry.values,
        }),
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A refusal to read a snapshot. Every variant names the snapshot or the path it
/// concerns and never quotes an answer value or a file's content.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot id is not a valid ULID: {0}")]
    Id(String),
    #[error("snapshot prefix must be 6 to 26 Crockford characters: {0}")]
    Prefix(String),
    #[error("snapshot.json is not valid JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("unsupported snapshot format version: {0}")]
    Format(u32),
    #[error("snapshot id {declared} does not match its ref {ref}")]
    IdRefMismatch {
        declared: SnapshotId,
        r#ref: SnapshotId,
    },
    #[error("snapshot source {declared} does not match template {template}")]
    Source { template: String, declared: String },
    #[error("invalid commit in snapshot: {0}")]
    Commit(String),
    #[error("invalid instant in snapshot: {0}")]
    Instant(String),
    #[error("invalid snapshot path {path}: {message}")]
    Path { path: String, message: String },
    #[error("snapshot path {path} carries ownership its origin cannot hold")]
    Ownership { path: String },
    #[error("snapshot paths do not equal the files under files/")]
    PathsFilesMismatch,
}

#[cfg(test)]
mod tests;
