// ---
// relationships:
//   implements: applied-record
// ---
//! The applied record read boundary and its application model.
use crate::plan::TargetPath;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SCHEMA_ID: &str = "https://wyrd.foo/toha/schemas/applied-record/1";
pub const SCHEMA: &str = include_str!("../../docs/specifications/applied-record-1.schema.yml");

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
    pub fn parse(text: &str) -> Result<Self, RecordError> {
        if text.is_empty() || text == "." {
            return Ok(Self::root());
        }
        // A non-root repo path obeys the same rules a target path does.
        TargetPath::parse(text).map_err(|message| RecordError::Path {
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
    pub fn parse(text: &str) -> Result<Self, RecordError> {
        let ok = text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit());
        if ok {
            Ok(Self(text.to_ascii_lowercase()))
        } else {
            Err(RecordError::Commit(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A lowercase SHA-256 content identity, distinct from a Git object id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn parse(text: &str) -> Result<Self, RecordError> {
        if text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Ok(Self(text.to_owned()))
        } else {
            Err(RecordError::ContentDigest(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The template revision a application was built at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// A git-addressed template resolved to this commit.
    Commit(CommitId),
    /// Content-addressed template bytes; never a Git object id.
    Content(ContentDigest),
    /// A folder template, which has no commit.
    Unversioned,
}

/// The instant every render of a project uses, frozen at the first application so a
/// template that renders a date stays byte-stable across updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenNow(jiff::Zoned, String);

impl FrozenNow {
    /// Wrap a zoned instant.
    pub fn new(instant: jiff::Zoned) -> Self {
        let text = instant.to_string();
        Self(instant, text)
    }

    pub fn parse(text: &str) -> Result<Self, RecordError> {
        text.parse::<jiff::Zoned>()
            .map(|instant| Self(instant, text.to_owned()))
            .map_err(|_| RecordError::Instant(text.to_owned()))
    }

    pub fn get(&self) -> &jiff::Zoned {
        &self.0
    }
}

impl fmt::Display for FrozenNow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.1)
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

/// One entry in an application's `paths`: a captured file and what Toha owns there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathOwnership {
    path: TargetPath,
    origin: Origin,
    blob: Option<gix::ObjectId>,
    mode: Option<gix::objs::tree::EntryKind>,
}

impl PathOwnership {
    /// A captured path with its ownership.
    pub fn new(path: TargetPath, origin: Origin) -> Self {
        Self {
            path,
            origin,
            blob: None,
            mode: None,
        }
    }

    pub fn with_blob(mut self, blob: gix::ObjectId) -> Self {
        if !matches!(self.origin, Origin::Edit { .. }) {
            self.blob = Some(blob);
        }
        self
    }
    pub fn with_mode(mut self, mode: gix::objs::tree::EntryKind) -> Self {
        if !matches!(self.origin, Origin::Edit { .. }) {
            self.mode = Some(mode);
        }
        self
    }
    pub fn mode(&self) -> Option<gix::objs::tree::EntryKind> {
        self.mode
    }
    pub fn blob(&self) -> Option<gix::ObjectId> {
        self.blob
    }

    pub fn path(&self) -> &TargetPath {
        &self.path
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }
}

/// One application in the committed record. Rendered file entries are transient;
/// only the recorded ownership and object ids cross the JSON boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct Application {
    template: String,
    source: String,
    revision: Revision,
    target: RepoPath,
    generated: FrozenNow,
    pub parts: Vec<String>,
    pub answers: BTreeMap<String, Value>,
    pub context: BTreeMap<String, Value>,
    paths: Vec<PathOwnership>,
    pub(crate) files: BTreeMap<String, (gix::ObjectId, gix::objs::tree::EntryKind)>,
}

impl fmt::Debug for Application {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Application")
            .field("template", &self.template)
            .field("target", &self.target)
            .field("revision", &self.revision)
            .field("answer_ids", &self.answers.keys().collect::<Vec<_>>())
            .field("context_names", &self.context.keys().collect::<Vec<_>>())
            .field("paths", &self.paths)
            .finish()
    }
}

impl Application {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        template: String,
        revision: Revision,
        target: RepoPath,
        generated: FrozenNow,
        answers: BTreeMap<String, Value>,
        context: BTreeMap<String, Value>,
        parts: Vec<String>,
        paths: Vec<PathOwnership>,
    ) -> Self {
        Self {
            source: source_identity(&template).to_owned(),
            template,
            revision,
            target,
            generated,
            answers,
            context,
            parts,
            paths,
            files: BTreeMap::new(),
        }
    }
    pub fn template(&self) -> &str {
        &self.template
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn revision(&self) -> &Revision {
        &self.revision
    }
    pub fn target(&self) -> &RepoPath {
        &self.target
    }
    pub fn generated(&self) -> &FrozenNow {
        &self.generated
    }
    pub fn paths(&self) -> &[PathOwnership] {
        &self.paths
    }
    pub fn submissions(&self) -> Vec<IndexMap<String, Value>> {
        vec![
            self.answers
                .iter()
                .map(|(id, v)| (id.clone(), v.clone()))
                .collect(),
        ]
    }
    pub fn to_value(&self) -> Value {
        let mut paths = serde_json::Map::new();
        for entry in &self.paths {
            let value = match entry.origin() {
                Origin::Toha => {
                    json!({"origin":"toha", "blob":entry.blob.map(|b| b.to_string()), "mode":entry.mode.map(mode_string)})
                }
                Origin::Hook => {
                    json!({"origin":"hook", "blob":entry.blob.map(|b| b.to_string()), "mode":entry.mode.map(mode_string)})
                }
                Origin::Edit { regions, values } => {
                    json!({"origin":"edit", "regions":regions,"values":values})
                }
            };
            paths.insert(entry.path.to_string(), value);
        }
        let mut value = json!({
            "target":self.target.as_str(), "template":self.template, "source":self.source,
            "commit":match &self.revision { Revision::Commit(c)=>Some(c.as_str()), _=>None },
            "generated":self.generated.to_string(), "parts":self.parts,
            "answers":self.answers, "context":self.context, "paths":paths
        });
        if let Revision::Content(digest) = &self.revision {
            value["content_digest"] = json!(digest.as_str());
        }
        value
    }
    fn from_value(value: &Value) -> Result<Self, RecordError> {
        let wire: ApplicationWire =
            serde_json::from_value(value.clone()).map_err(RecordError::Json)?;
        if source_identity(&wire.template) != wire.source {
            return Err(RecordError::Source {
                template: wire.template,
                declared: wire.source,
            });
        }
        let revision = match (wire.commit, wire.content_digest) {
            (Some(commit), None) => Revision::Commit(CommitId::parse(&commit)?),
            (None, Some(digest)) => Revision::Content(ContentDigest::parse(&digest)?),
            (None, None) => Revision::Unversioned,
            _ => return Err(RecordError::ConflictingRevision),
        };
        let mut paths = Vec::new();
        for (raw, entry) in wire.paths {
            let path = TargetPath::parse(&raw).map_err(|message| RecordError::Path {
                path: raw.clone(),
                message,
            })?;
            let origin = match entry.origin.as_str() {
                "toha" => Origin::Toha,
                "hook" => Origin::Hook,
                _ => Origin::Edit {
                    regions: entry.regions,
                    values: entry.values,
                },
            };
            let mut owned = PathOwnership::new(path, origin);
            if let Some(blob) = entry.blob {
                owned =
                    owned.with_blob(gix::ObjectId::from_hex(blob.as_bytes()).map_err(git_error)?);
            }
            if let Some(mode) = entry.mode {
                owned = owned.with_mode(match mode.as_str() {
                    "100644" => gix::objs::tree::EntryKind::Blob,
                    "100755" => gix::objs::tree::EntryKind::BlobExecutable,
                    "120000" => gix::objs::tree::EntryKind::Link,
                    _ => unreachable!("schema checked"),
                });
            }
            paths.push(owned);
        }
        Ok(Self::new(
            wire.template,
            revision,
            RepoPath::parse(&wire.target)?,
            FrozenNow::parse(&wire.generated)?,
            wire.answers,
            wire.context,
            wire.parts,
            paths,
        ))
    }
}
#[derive(Deserialize)]
struct ApplicationWire {
    template: String,
    source: String,
    commit: Option<String>,
    content_digest: Option<String>,
    target: String,
    generated: String,
    parts: Vec<String>,
    answers: BTreeMap<String, Value>,
    context: BTreeMap<String, Value>,
    paths: BTreeMap<String, PathWire>,
}
#[derive(Deserialize)]
struct PathWire {
    origin: String,
    blob: Option<String>,
    mode: Option<String>,
    #[serde(default)]
    regions: Vec<String>,
    #[serde(default)]
    values: Vec<String>,
}

fn mode_string(mode: gix::objs::tree::EntryKind) -> &'static str {
    match mode {
        gix::objs::tree::EntryKind::Blob => "100644",
        gix::objs::tree::EntryKind::BlobExecutable => "100755",
        gix::objs::tree::EntryKind::Link => "120000",
        gix::objs::tree::EntryKind::Tree => "040000",
        gix::objs::tree::EntryKind::Commit => "160000",
    }
}

/// A validated record. Unknown members follow the published line's schema.
#[derive(Debug, Clone, Default)]
pub struct AppliedRecord {
    applications: Vec<Application>,
}
impl AppliedRecord {
    pub fn parse(bytes: &[u8]) -> Result<Self, RecordError> {
        let value: Value = serde_json::from_slice(bytes).map_err(RecordError::Json)?;
        validate_schema(&value)?;
        let mut applications = Vec::new();
        let mut pairs = BTreeSet::new();
        for app in value["applications"].as_array().expect("schema checked") {
            let app = Application::from_value(app)?;
            if !pairs.insert((app.target.clone(), app.source.clone())) {
                return Err(RecordError::Duplicate);
            }
            applications.push(app);
        }
        Ok(Self { applications })
    }
    pub fn applications(&self) -> &[Application] {
        &self.applications
    }
    pub fn replace(&mut self, app: Application) {
        self.applications
            .retain(|a| a.target != app.target || a.source != app.source);
        self.applications.push(app);
        self.applications
            .sort_by(|a, b| (&a.target, &a.source).cmp(&(&b.target, &b.source)));
    }
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, RecordError> {
        let mut applications: Vec<_> = self.applications.iter().collect();
        applications.sort_by(|a, b| (&a.target, &a.source).cmp(&(&b.target, &b.source)));
        let value = json!({"$schema":SCHEMA_ID,"applications":applications.into_iter().map(Application::to_value).collect::<Vec<_>>()});
        validate_schema(&value)?;
        let mut bytes = serde_json::to_vec_pretty(&value).map_err(RecordError::Json)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}
fn validate_schema(value: &Value) -> Result<(), RecordError> {
    static VALIDATOR: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_norway::from_str(SCHEMA).expect("bundled applied record schema parses");
        jsonschema::validator_for(&schema).expect("bundled applied record schema compiles")
    });
    validator
        .validate(value)
        .map_err(|error| RecordError::Schema(error.instance_path().to_string()))
}

pub(crate) struct CapturedBlob {
    pub path: TargetPath,
    pub kind: gix::objs::tree::EntryKind,
    pub oid: gix::ObjectId,
}
fn git_error(err: impl fmt::Display) -> RecordError {
    RecordError::Git(err.to_string())
}
pub fn source_identity(formal: &str) -> String {
    if formal.starts_with('/')
        || formal.starts_with('\\')
        || (formal.as_bytes().get(1) == Some(&b':') && formal.as_bytes()[0].is_ascii_alphabetic())
    {
        return formal.to_owned();
    }
    let (address, path) = formal
        .split_once('#')
        .map_or((formal, None), |(a, p)| (a, Some(p)));
    let source = address.rsplit_once('@').map_or(address, |(s, _)| s);
    match path {
        Some(path) => format!("{source}#{path}"),
        None => source.to_owned(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("applied record is not valid JSON (line {line}, column {column})", line=.0.line(), column=.0.column())]
    Json(#[source] serde_json::Error),
    #[error("applied record fails its schema at {0}")]
    Schema(String),
    #[error("applied record repeats a source and target pair")]
    Duplicate,
    #[error("record source {declared} does not match template {template}")]
    Source { template: String, declared: String },
    #[error("invalid template commit: {0}")]
    Commit(String),
    #[error("invalid content digest: {0}")]
    ContentDigest(String),
    #[error("application cannot carry both a commit and content digest")]
    ConflictingRevision,
    #[error("invalid render instant: {0}")]
    Instant(String),
    #[error("invalid record path {path}: {message}")]
    Path { path: String, message: String },
    #[error("no application of {0}; use apply TEMPLATE PATH --baseline to re-baseline")]
    Unknown(String),
    #[error("cannot read applied record: {0}")]
    Git(String),
}
