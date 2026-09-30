// ---
// relationships:
//   implements: architecture
// ---
//! Content injection: bounded mutation of an existing target file.
//!
//! Two ownership mechanisms, one per file family:
//!
//! - a visible checksum-marked region for non-JSON text, and
//! - a typed value at a parsed path for JSON, JSONC, and JSON5, through the
//!   embedded `jsonc-parser` concrete syntax tree.
//!
//! Both resolvers are pure: they take the current bytes and a planned edit and
//! return a complete candidate file. They perform no I/O. Apply groups a
//! target's edits, folds them over one in-memory buffer, and writes once.

use std::fmt;

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstNode, CstRootNode};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::plan::TargetPath;

/// One planned edit of a target file: a visible region or a typed JSON value.
#[derive(Debug)]
pub enum PlannedEdit {
    Region(PlannedRegionEdit),
    JsonValue(PlannedJsonEdit),
}
impl PlannedEdit {
    /// The target this edit mutates.
    pub fn path(&self) -> &TargetPath {
        match self {
            Self::Region(edit) => &edit.path,
            Self::JsonValue(edit) => &edit.path,
        }
    }
}

/// A visible managed region in a non-JSON text target. Toha owns the marker
/// pair and the checksummed body between them.
#[derive(Debug)]
pub struct PlannedRegionEdit {
    pub path: TargetPath,
    pub region: RegionKey,
    /// The rendered body Toha owns between the markers.
    pub body: String,
    pub marker: MarkerStyle,
    /// The bootstrap anchor, used only when the markers do not yet exist.
    pub anchor: Option<Anchor>,
    pub create: bool,
    /// The support file the body was rendered from, or `None` for an inline
    /// `content` body. Retained for diagnostics and parity with `files:`.
    pub source: Option<std::path::PathBuf>,
}

/// A typed value at a parsed path in a JSON-family target. Toha owns the value
/// at the path and converges it on every apply.
#[derive(Debug)]
pub struct PlannedJsonEdit {
    pub path: TargetPath,
    pub json_path: JsonPath,
    pub desired: Value,
    pub format: JsonFormat,
    pub create: bool,
}

/// The region key: lowercase ASCII letters, digits, `_`, and `-`. Unique per
/// target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionKey(String);
impl RegionKey {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(format!(
                "invalid region key: {value} (use lowercase letters, digits, `_`, and `-`)"
            ));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for RegionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The comment syntax that carries a region's markers. `suffix` is empty for a
/// line comment and the closing delimiter for a block comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerStyle {
    pub prefix: String,
    pub suffix: String,
}
impl MarkerStyle {
    /// A line comment with the given prefix and no suffix.
    pub fn line(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            suffix: String::new(),
        }
    }
    /// A block comment with an opening and closing delimiter.
    pub fn block(open: &str, close: &str) -> Self {
        Self {
            prefix: open.to_owned(),
            suffix: format!(" {close}"),
        }
    }
    /// Infers a comment style from a target's file name, or `None` when the
    /// extension is unknown and the author must supply a `marker` override.
    pub fn infer(path: &TargetPath) -> Option<Self> {
        let name = path.as_path().file_name()?.to_str()?;
        let lower = name.to_ascii_lowercase();
        // Files identified by whole name rather than extension.
        match lower.as_str() {
            "makefile" | "dockerfile" | "gitignore" | ".gitignore" | "dockerignore"
            | ".dockerignore" | ".editorconfig" | ".env" | "cargo.lock" => {
                return Some(Self::line("#"));
            }
            _ => {}
        }
        let ext = lower.rsplit_once('.').map(|(_, ext)| ext)?;
        let style = match ext {
            "sh" | "bash" | "zsh" | "fish" | "ksh" | "py" | "pyw" | "rb" | "pl" | "pm" | "r"
            | "jl" | "ex" | "exs" | "nim" | "cr" | "toml" | "yaml" | "yml" | "ini" | "cfg"
            | "conf" | "properties" | "tf" | "hcl" | "mk" | "env" | "gitignore" | "gitconfig"
            | "dockerfile" | "ps1" | "psm1" | "awk" | "tcl" | "rake" | "gemspec" => Self::line("#"),
            "rs" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts" | "c" | "h"
            | "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" | "java" | "kt" | "kts" | "scala"
            | "go" | "swift" | "cs" | "php" | "dart" | "proto" | "glsl" | "zig" | "v" | "rs2"
            | "groovy" | "gradle" | "less" | "scss" | "sass" | "styl" => Self::line("//"),
            "sql" | "lua" | "hs" | "elm" | "ada" | "adb" | "ads" | "vhd" | "vhdl" | "hql" => {
                Self::line("--")
            }
            "clj" | "cljs" | "cljc" | "edn" | "lisp" | "lsp" | "el" | "scm" | "asm" | "s" => {
                Self::line(";")
            }
            "tex" | "sty" | "cls" | "bib" | "erl" | "hrl" | "mat" | "m" => Self::line("%"),
            "html" | "htm" | "xhtml" | "xml" | "svg" | "vue" | "xaml" | "md" | "markdown"
            | "mdx" | "rss" | "atom" | "resx" | "plist" => Self::block("<!--", "-->"),
            "css" => Self::block("/*", "*/"),
            _ => return None,
        };
        Some(style)
    }

    fn begin(&self, key: &RegionKey) -> String {
        format!("{} >>> toha:region {key} >>>{}", self.prefix, self.suffix)
    }
    fn end(&self, key: &RegionKey, sha: &str) -> String {
        format!(
            "{} <<< toha:end {key} sha256:{sha} <<<{}",
            self.prefix, self.suffix
        )
    }
    /// The fixed prefix and suffix that bracket the checksum in an end marker,
    /// used to recognise an existing end marker and read its recorded checksum.
    fn end_fixes(&self, key: &RegionKey) -> (String, String) {
        (
            format!("{} <<< toha:end {key} sha256:", self.prefix),
            format!(" <<<{}", self.suffix),
        )
    }
}

/// Where a region is first placed when its markers do not yet exist.
#[derive(Debug, Clone)]
pub struct Anchor {
    /// The rendered line to place the region after.
    pub after: String,
    pub occurrence: Occurrence,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occurrence {
    /// Exactly one matching line; more than one is an ambiguity error.
    Only,
    First,
    Last,
}

/// The JSON-family format selected from the target extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFormat {
    Json,
    Jsonc,
    Json5,
}
impl JsonFormat {
    /// The named `ParseOptions` bundle. Every field is set explicitly rather
    /// than inheriting the dependency's all-enabled default: `Json` disables
    /// every relaxation, `Jsonc` enables comments and trailing commas only, and
    /// `Json5` enables the full permissive bundle. The `Json5` bundle is a
    /// JSON5-compatible superset — it also permits missing commas — not a strict
    /// JSON5 grammar validator.
    pub fn parse_options(self) -> ParseOptions {
        match self {
            Self::Json => ParseOptions {
                allow_comments: false,
                allow_loose_object_property_names: false,
                allow_trailing_commas: false,
                allow_missing_commas: false,
                allow_single_quoted_strings: false,
                allow_hexadecimal_numbers: false,
                allow_unary_plus_numbers: false,
                allow_bare_decimal_point_numbers: false,
                allow_non_finite_numbers: false,
                allow_extended_string_escapes: false,
            },
            Self::Jsonc => ParseOptions {
                allow_comments: true,
                allow_loose_object_property_names: false,
                allow_trailing_commas: true,
                allow_missing_commas: false,
                allow_single_quoted_strings: false,
                allow_hexadecimal_numbers: false,
                allow_unary_plus_numbers: false,
                allow_bare_decimal_point_numbers: false,
                allow_non_finite_numbers: false,
                allow_extended_string_escapes: false,
            },
            Self::Json5 => ParseOptions {
                allow_comments: true,
                allow_loose_object_property_names: true,
                allow_trailing_commas: true,
                allow_missing_commas: true,
                allow_single_quoted_strings: true,
                allow_hexadecimal_numbers: true,
                allow_unary_plus_numbers: true,
                allow_bare_decimal_point_numbers: true,
                allow_non_finite_numbers: true,
                allow_extended_string_escapes: true,
            },
        }
    }
}

/// A parsed typed path: dot-separated object keys and bracketed array indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath(Vec<JsonPathSegment>);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathSegment {
    Key(String),
    Index(usize),
}
impl JsonPath {
    /// Parses `routes[0].name` into segments. `.` separates object keys, `[n]`
    /// selects an array index, and a backslash escapes `.`, `[`, `]`, and `\` in
    /// a key. The path must begin with a key.
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut segments = Vec::new();
        let mut chars = value.chars().peekable();
        // A path begins with a key.
        if value.is_empty() {
            return Err("empty path".into());
        }
        if chars.peek() == Some(&'[') {
            return Err("path must begin with a key".into());
        }
        loop {
            match chars.peek() {
                None => break,
                Some('[') => {
                    chars.next();
                    let mut digits = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ']' {
                            break;
                        }
                        digits.push(c);
                        chars.next();
                    }
                    if chars.next() != Some(']') {
                        return Err(format!("unclosed array index in path: {value}"));
                    }
                    let index: usize = digits
                        .parse()
                        .map_err(|_| format!("invalid array index `{digits}` in path: {value}"))?;
                    segments.push(JsonPathSegment::Index(index));
                    // After an index, a `.` introduces the next key; `[` another
                    // index; anything else is the end.
                    if chars.peek() == Some(&'.') {
                        chars.next();
                        segments.push(read_key(&mut chars, value)?);
                    }
                }
                Some('.') => {
                    // A leading key was already read; a `.` here introduces the
                    // next key.
                    chars.next();
                    segments.push(read_key(&mut chars, value)?);
                }
                Some(_) => {
                    segments.push(read_key(&mut chars, value)?);
                }
            }
        }
        if segments.is_empty() {
            return Err(format!("empty path: {value}"));
        }
        Ok(Self(segments))
    }
    pub fn segments(&self) -> &[JsonPathSegment] {
        &self.0
    }
    /// Whether this path is an ancestor of, a descendant of, or equal to
    /// `other` — the overlap that makes two edits on one target conflict.
    pub fn overlaps(&self, other: &Self) -> bool {
        let shorter = self.0.len().min(other.0.len());
        self.0[..shorter] == other.0[..shorter]
    }
}
impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            match segment {
                JsonPathSegment::Key(key) => {
                    if i > 0 {
                        write!(f, ".")?;
                    }
                    for ch in key.chars() {
                        if matches!(ch, '.' | '[' | ']' | '\\') {
                            write!(f, "\\")?;
                        }
                        write!(f, "{ch}")?;
                    }
                }
                JsonPathSegment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

/// Reads one key from the path, up to the next unescaped `.`, `[`, or end.
fn read_key(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    value: &str,
) -> Result<JsonPathSegment, String> {
    let mut key = String::new();
    while let Some(&c) = chars.peek() {
        match c {
            '.' | '[' => break,
            ']' => return Err(format!("unexpected `]` in path: {value}")),
            '\\' => {
                chars.next();
                match chars.next() {
                    Some(escaped @ ('.' | '[' | ']' | '\\')) => key.push(escaped),
                    _ => return Err(format!("invalid escape in path: {value}")),
                }
            }
            _ => {
                key.push(c);
                chars.next();
            }
        }
    }
    if key.is_empty() {
        return Err(format!("empty key in path: {value}"));
    }
    Ok(JsonPathSegment::Key(key))
}

/// The complete candidate file a resolver returns, or the signal that nothing
/// changed or that the operator drifted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditResolution {
    Unchanged,
    Write(Vec<u8>),
    /// The operator changed the owned region bytes; `forced` is the replacement
    /// a `--force` apply would write.
    Drift {
        forced: Vec<u8>,
    },
}

/// How an edit reports in a dry run: it would place a new region or value,
/// change an existing one, leave it unchanged, or refuse an operator's drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditReport {
    Inject,
    Update,
    Unchanged,
    Drift,
}

#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error("target missing for region `{0}`; declare create: true to create it")]
    TargetMissing(RegionKey),
    #[error("target is not UTF-8 text for region `{0}`")]
    NotUtf8(RegionKey),
    #[error("malformed or duplicate markers for region `{0}`")]
    MalformedMarkers(RegionKey),
    #[error("no line matches the anchor for region `{0}`")]
    AnchorMissing(RegionKey),
    #[error("the anchor for region `{0}` matches more than one line")]
    AnchorAmbiguous(RegionKey),
}

#[derive(Debug, thiserror::Error)]
pub enum JsonEditError {
    #[error("target missing for `{path}`; declare create: true to create it")]
    TargetMissing { path: JsonPath },
    #[error("target is not UTF-8 text for `{path}`")]
    NotUtf8 { path: JsonPath },
    #[error("malformed {format:?} source for `{path}`: {message}")]
    Parse {
        path: JsonPath,
        format: JsonFormat,
        message: String,
    },
    #[error("invalid traversal for `{path}`: {message}")]
    Traversal { path: JsonPath, message: String },
    #[error("array index {index} out of range for `{path}`")]
    IndexOutOfRange { path: JsonPath, index: usize },
}

/// Converts a `serde_json::Value` into the dependency's `CstInputValue`. The
/// dependency has no `From<serde_json::Value>`, so Toha owns this recursive
/// mapping. Numbers use `Number::to_string()`, and objects retain the map's
/// iteration order (sorted, since Toha's `serde_json` has no `preserve_order`).
pub fn json_value_to_cst_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(items) => {
            CstInputValue::Array(items.iter().map(json_value_to_cst_input).collect())
        }
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), json_value_to_cst_input(v)))
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Region resolver
// ---------------------------------------------------------------------------

/// Resolves a region edit into a complete candidate file. Never performs I/O.
pub fn resolve_region_edit(
    current: Option<&[u8]>,
    edit: &PlannedRegionEdit,
) -> Result<EditResolution, RegionError> {
    Ok(match plan_region(current, edit)? {
        RegionOutcome::Unchanged => EditResolution::Unchanged,
        RegionOutcome::Write(bytes) => EditResolution::Write(bytes),
        RegionOutcome::Drift(forced) => EditResolution::Drift { forced },
    })
}

/// Classifies a region edit for a dry run without producing bytes for a write.
pub fn report_region_edit(
    current: Option<&[u8]>,
    edit: &PlannedRegionEdit,
) -> Result<EditReport, RegionError> {
    Ok(match plan_region(current, edit)? {
        RegionOutcome::Unchanged => EditReport::Unchanged,
        RegionOutcome::Write(_) => report_region_kind(current, edit)?,
        RegionOutcome::Drift(_) => EditReport::Drift,
    })
}

/// Whether a write would place a new region (`Inject`) or replace an existing
/// one (`Update`), read from whether the markers already exist.
fn report_region_kind(
    current: Option<&[u8]>,
    edit: &PlannedRegionEdit,
) -> Result<EditReport, RegionError> {
    let present = match current {
        None => false,
        Some(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| RegionError::NotUtf8(edit.region.clone()))?;
            find_region(text, &edit.marker, &edit.region)?.is_some()
        }
    };
    Ok(if present {
        EditReport::Update
    } else {
        EditReport::Inject
    })
}

enum RegionOutcome {
    Unchanged,
    Write(Vec<u8>),
    Drift(Vec<u8>),
}

/// The located begin/end marker lines of an existing region: the byte offset of
/// the begin line's start, the offset just past the begin line's terminator,
/// the offset of the end line's start, the offset just past the end line's
/// terminator, and the checksum recorded in the end marker.
struct Located {
    begin_start: usize,
    body_start: usize,
    end_start: usize,
    end_line_end: usize,
    recorded: String,
}

fn plan_region(
    current: Option<&[u8]>,
    edit: &PlannedRegionEdit,
) -> Result<RegionOutcome, RegionError> {
    let bytes = match current {
        Some(bytes) => bytes,
        None => {
            if !edit.create {
                return Err(RegionError::TargetMissing(edit.region.clone()));
            }
            &[]
        }
    };
    let text = std::str::from_utf8(bytes).map_err(|_| RegionError::NotUtf8(edit.region.clone()))?;
    let body = normalize_body(&edit.body);
    let sha = checksum(&body);
    let begin = edit.marker.begin(&edit.region);
    let end = edit.marker.end(&edit.region, &sha);

    match find_region(text, &edit.marker, &edit.region)? {
        Some(located) => {
            let current_body = &text[located.body_start..located.end_start];
            if current_body == body {
                return Ok(RegionOutcome::Unchanged);
            }
            let replacement = format!("{begin}\n{body}{end}\n");
            let mut out = String::with_capacity(text.len() + replacement.len());
            out.push_str(&text[..located.begin_start]);
            out.push_str(&replacement);
            out.push_str(&text[located.end_line_end..]);
            let bytes = out.into_bytes();
            if checksum(current_body) == located.recorded {
                // Toha wrote the current body; the template body changed.
                Ok(RegionOutcome::Write(bytes))
            } else {
                // The operator edited the owned bytes.
                Ok(RegionOutcome::Drift(bytes))
            }
        }
        None => {
            let block = format!("{begin}\n{body}{end}\n");
            let out = place_region(text, edit, &block)?;
            Ok(RegionOutcome::Write(out.into_bytes()))
        }
    }
}

/// Normalises a region body to end with exactly one newline so the end marker
/// sits on its own line; an empty body stays empty.
fn normalize_body(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else if body.ends_with('\n') {
        body.to_owned()
    } else {
        format!("{body}\n")
    }
}

fn checksum(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Places a first-time region at its anchor, or at end of file when it has no
/// anchor.
fn place_region(text: &str, edit: &PlannedRegionEdit, block: &str) -> Result<String, RegionError> {
    match &edit.anchor {
        Some(anchor) => {
            let insert_at = anchor_offset(text, anchor, &edit.region)?;
            let mut out = String::with_capacity(text.len() + block.len());
            out.push_str(&text[..insert_at]);
            out.push_str(block);
            out.push_str(&text[insert_at..]);
            Ok(out)
        }
        None => {
            let mut out = String::with_capacity(text.len() + block.len() + 1);
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(block);
            Ok(out)
        }
    }
}

/// The byte offset just past the anchor line's terminator, where a first
/// placement inserts the region.
fn anchor_offset(text: &str, anchor: &Anchor, key: &RegionKey) -> Result<usize, RegionError> {
    let matched: Vec<Line> = scan_lines(text)
        .into_iter()
        .filter(|line| line_content(text, line) == anchor.after)
        .collect();
    let chosen = match anchor.occurrence {
        Occurrence::Only => match matched.as_slice() {
            [] => return Err(RegionError::AnchorMissing(key.clone())),
            [one] => one,
            _ => return Err(RegionError::AnchorAmbiguous(key.clone())),
        },
        Occurrence::First => matched
            .first()
            .ok_or_else(|| RegionError::AnchorMissing(key.clone()))?,
        Occurrence::Last => matched
            .last()
            .ok_or_else(|| RegionError::AnchorMissing(key.clone()))?,
    };
    Ok(chosen.end)
}

/// Finds the single begin/end marker pair for a region key, or `None` when no
/// markers exist. Any other shape — a lone begin, a lone end, a duplicate, or an
/// end before a begin — is a malformed-markers error.
fn find_region(
    text: &str,
    style: &MarkerStyle,
    key: &RegionKey,
) -> Result<Option<Located>, RegionError> {
    let begin = style.begin(key);
    let (end_prefix, end_suffix) = style.end_fixes(key);
    let lines = scan_lines(text);
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let content = line_content(text, line);
        if content == begin {
            begins.push(i);
        } else if let Some(recorded) = read_end(content, &end_prefix, &end_suffix) {
            ends.push((i, recorded.to_owned()));
        }
    }
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([bi], [(ei, recorded)]) if bi < ei => {
            let begin_line = &lines[*bi];
            let end_line = &lines[*ei];
            Ok(Some(Located {
                begin_start: begin_line.start,
                body_start: begin_line.end,
                end_start: end_line.start,
                end_line_end: end_line.end,
                recorded: recorded.clone(),
            }))
        }
        _ => Err(RegionError::MalformedMarkers(key.clone())),
    }
}

/// Reads the recorded checksum from an end-marker line, or `None` when the line
/// is not an end marker for this region.
fn read_end<'a>(content: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let content = content.trim_end();
    let inner = content.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (!inner.is_empty() && inner.bytes().all(|b| b.is_ascii_hexdigit())).then_some(inner)
}

/// One line of the source: the byte offset of its first character, the offset
/// just past its last content character, and the offset just past its
/// terminator (equal to `content_end` for a final line with no terminator).
#[derive(Clone)]
struct Line {
    start: usize,
    content_end: usize,
    end: usize,
}

fn scan_lines(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let mut content_end = i;
            if content_end > start && bytes[content_end - 1] == b'\r' {
                content_end -= 1;
            }
            lines.push(Line {
                start,
                content_end,
                end: i + 1,
            });
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        lines.push(Line {
            start,
            content_end: bytes.len(),
            end: bytes.len(),
        });
    }
    lines
}

fn line_content<'a>(text: &'a str, line: &Line) -> &'a str {
    &text[line.start..line.content_end]
}

// ---------------------------------------------------------------------------
// JSON-family resolver
// ---------------------------------------------------------------------------

/// Resolves a JSON-family edit into a complete candidate file. Never performs
/// I/O. JSON ownership is convergent: it never returns `Drift`.
pub fn resolve_json_edit(
    current: Option<&[u8]>,
    edit: &PlannedJsonEdit,
) -> Result<EditResolution, JsonEditError> {
    Ok(match plan_json(current, edit)? {
        JsonOutcome::Unchanged => EditResolution::Unchanged,
        JsonOutcome::Write { bytes, .. } => EditResolution::Write(bytes),
    })
}

/// Classifies a JSON-family edit for a dry run.
pub fn report_json_edit(
    current: Option<&[u8]>,
    edit: &PlannedJsonEdit,
) -> Result<EditReport, JsonEditError> {
    Ok(match plan_json(current, edit)? {
        JsonOutcome::Unchanged => EditReport::Unchanged,
        JsonOutcome::Write { existed, .. } => {
            if existed {
                EditReport::Update
            } else {
                EditReport::Inject
            }
        }
    })
}

enum JsonOutcome {
    Unchanged,
    Write { bytes: Vec<u8>, existed: bool },
}

/// A container the traversal descends through.
enum Container {
    Object(jsonc_parser::cst::CstObject),
    Array(jsonc_parser::cst::CstArray),
}

fn plan_json(current: Option<&[u8]>, edit: &PlannedJsonEdit) -> Result<JsonOutcome, JsonEditError> {
    let owned;
    let bytes: &[u8] = match current {
        Some(bytes) => bytes,
        None => {
            if !edit.create {
                return Err(JsonEditError::TargetMissing {
                    path: edit.json_path.clone(),
                });
            }
            owned = b"{}".to_vec();
            &owned
        }
    };
    let text = std::str::from_utf8(bytes).map_err(|_| JsonEditError::NotUtf8 {
        path: edit.json_path.clone(),
    })?;
    // Strict JSON has no comments, but the `jsonc-parser` concrete syntax tree
    // keeps a comment as a trivia token regardless of `allow_comments`, so it
    // does not reject one. `serde_json` is a strict RFC 8259 parser and is
    // already a dependency; validate a `.json` target through it before any CST
    // mutation so a comment, a trailing comma, or any other relaxation is
    // refused and nothing is written. JSONC and JSON5 keep their own policies.
    if edit.format == JsonFormat::Json {
        serde_json::from_str::<Value>(text).map_err(|e| JsonEditError::Parse {
            path: edit.json_path.clone(),
            format: edit.format,
            message: e.to_string(),
        })?;
    }
    let root = CstRootNode::parse(text, &edit.format.parse_options()).map_err(|e| {
        JsonEditError::Parse {
            path: edit.json_path.clone(),
            format: edit.format,
            message: e.to_string(),
        }
    })?;
    // A path begins with a key, so the root must be an object.
    let root_obj = match root.value() {
        Some(node) => node.as_object().ok_or_else(|| JsonEditError::Traversal {
            path: edit.json_path.clone(),
            message: "root is not a JSON object".into(),
        })?,
        None => root.object_value_or_set(),
    };
    let segments = edit.json_path.segments();
    let (last, parents) = segments
        .split_last()
        .expect("a parsed path has at least one segment");

    // Walk to the leaf's parent container, creating missing object parents.
    let mut container = Container::Object(root_obj);
    for (i, seg) in parents.iter().enumerate() {
        let need_array = matches!(segments[i + 1], JsonPathSegment::Index(_));
        container = descend(container, seg, need_array, edit)?;
    }

    let existed = match (container, last) {
        (Container::Object(obj), JsonPathSegment::Key(k)) => match obj.get(k) {
            Some(prop) => {
                let current_value = prop.value().and_then(|n| n.to_serde_value());
                if current_value.as_ref() == Some(&edit.desired) {
                    return Ok(JsonOutcome::Unchanged);
                }
                prop.set_value(json_value_to_cst_input(&edit.desired));
                true
            }
            None => {
                obj.append(k, json_value_to_cst_input(&edit.desired));
                false
            }
        },
        (Container::Array(arr), JsonPathSegment::Index(idx)) => {
            let elements = arr.elements();
            let element = elements
                .get(*idx)
                .ok_or_else(|| JsonEditError::IndexOutOfRange {
                    path: edit.json_path.clone(),
                    index: *idx,
                })?
                .clone();
            if element.to_serde_value().as_ref() == Some(&edit.desired) {
                return Ok(JsonOutcome::Unchanged);
            }
            replace_node(element, json_value_to_cst_input(&edit.desired));
            true
        }
        (Container::Object(_), JsonPathSegment::Index(_)) => {
            return Err(JsonEditError::Traversal {
                path: edit.json_path.clone(),
                message: "expected an array but found an object".into(),
            });
        }
        (Container::Array(_), JsonPathSegment::Key(_)) => {
            return Err(JsonEditError::Traversal {
                path: edit.json_path.clone(),
                message: "expected an object but found an array".into(),
            });
        }
    };
    Ok(JsonOutcome::Write {
        bytes: root.to_string().into_bytes(),
        existed,
    })
}

/// Descends one parent segment, creating a missing object parent but never a
/// missing array.
fn descend(
    container: Container,
    seg: &JsonPathSegment,
    need_array: bool,
    edit: &PlannedJsonEdit,
) -> Result<Container, JsonEditError> {
    let traversal = |message: &str| JsonEditError::Traversal {
        path: edit.json_path.clone(),
        message: message.into(),
    };
    match (container, seg) {
        (Container::Object(obj), JsonPathSegment::Key(k)) => match obj.get(k) {
            Some(prop) => match prop.value() {
                Some(node) => {
                    if need_array {
                        node.as_array()
                            .map(Container::Array)
                            .ok_or_else(|| traversal("expected an array in the path"))
                    } else {
                        node.as_object()
                            .map(Container::Object)
                            .ok_or_else(|| traversal("expected an object in the path"))
                    }
                }
                None => Err(traversal("malformed property in the path")),
            },
            None => {
                if need_array {
                    Err(traversal("an array in the path does not exist"))
                } else {
                    Ok(Container::Object(obj.object_value_or_set(k)))
                }
            }
        },
        (Container::Array(arr), JsonPathSegment::Index(idx)) => {
            let elements = arr.elements();
            let element = elements
                .get(*idx)
                .ok_or(JsonEditError::IndexOutOfRange {
                    path: edit.json_path.clone(),
                    index: *idx,
                })?
                .clone();
            if need_array {
                element
                    .as_array()
                    .map(Container::Array)
                    .ok_or_else(|| traversal("expected an array in the path"))
            } else {
                element
                    .as_object()
                    .map(Container::Object)
                    .ok_or_else(|| traversal("expected an object in the path"))
            }
        }
        (Container::Object(_), JsonPathSegment::Index(_)) => {
            Err(traversal("expected an array but found an object"))
        }
        (Container::Array(_), JsonPathSegment::Key(_)) => {
            Err(traversal("expected an object but found an array"))
        }
    }
}

/// Replaces an arbitrary CST value node with the input value, dispatching over
/// its concrete kind since `CstNode` has no direct `replace_with`.
fn replace_node(node: CstNode, input: CstInputValue) {
    if let Some(object) = node.as_object() {
        object.replace_with(input);
    } else if let Some(array) = node.as_array() {
        array.replace_with(input);
    } else if let Some(string) = node.as_string_lit() {
        string.replace_with(input);
    } else if let Some(number) = node.as_number_lit() {
        number.replace_with(input);
    } else if let Some(boolean) = node.as_boolean_lit() {
        boolean.replace_with(input);
    } else if let Some(null) = node.as_null_keyword() {
        null.replace_with(input);
    } else if let Some(word) = node.as_word_lit() {
        word.replace_with(input);
    }
}

// ---------------------------------------------------------------------------
// Retraction — removing an ownership a later template version dropped
// ---------------------------------------------------------------------------

/// Remove the managed region `region` from `current`, leaving the operator's
/// surrounding content. Returns the bytes unchanged when the region is absent,
/// so retraction is idempotent.
pub fn retract_region(
    current: &[u8],
    region: &RegionKey,
    marker: &MarkerStyle,
) -> Result<Vec<u8>, RegionError> {
    let text = std::str::from_utf8(current).map_err(|_| RegionError::NotUtf8(region.clone()))?;
    match find_region(text, marker, region)? {
        Some(located) => {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..located.begin_start]);
            out.push_str(&text[located.end_line_end..]);
            Ok(out.into_bytes())
        }
        None => Ok(current.to_vec()),
    }
}

/// Remove the value Toha owns at `json_path` from a JSON-family target, applying
/// the same strict `.json` check the resolver applies. Returns the bytes
/// unchanged when the value is absent. Removing several array elements requires
/// calling this from the highest index down so earlier indexes stay valid.
pub fn retract_json_value(
    current: &[u8],
    json_path: &JsonPath,
    format: JsonFormat,
) -> Result<Vec<u8>, JsonEditError> {
    let text = std::str::from_utf8(current).map_err(|_| JsonEditError::NotUtf8 {
        path: json_path.clone(),
    })?;
    if format == JsonFormat::Json {
        serde_json::from_str::<Value>(text).map_err(|e| JsonEditError::Parse {
            path: json_path.clone(),
            format,
            message: e.to_string(),
        })?;
    }
    let root =
        CstRootNode::parse(text, &format.parse_options()).map_err(|e| JsonEditError::Parse {
            path: json_path.clone(),
            format,
            message: e.to_string(),
        })?;
    let Some(root_obj) = root.value().and_then(|node| node.as_object()) else {
        return Ok(current.to_vec());
    };

    let segments = json_path.segments();
    let (last, parents) = segments
        .split_last()
        .expect("a parsed path has at least one segment");

    let mut container = Container::Object(root_obj);
    for (i, seg) in parents.iter().enumerate() {
        let need_array = matches!(segments[i + 1], JsonPathSegment::Index(_));
        container = match navigate(container, seg, need_array) {
            Some(next) => next,
            None => return Ok(current.to_vec()),
        };
    }

    match (container, last) {
        (Container::Object(obj), JsonPathSegment::Key(k)) => {
            if let Some(prop) = obj.get(k) {
                prop.remove();
            }
        }
        (Container::Array(arr), JsonPathSegment::Index(idx)) => {
            if let Some(element) = arr.elements().get(*idx) {
                element.clone().remove();
            }
        }
        _ => return Ok(current.to_vec()),
    }
    Ok(root.to_string().into_bytes())
}

/// Descend one parent segment without creating anything, returning `None` when
/// the path does not exist so retraction leaves the file unchanged.
fn navigate(container: Container, seg: &JsonPathSegment, need_array: bool) -> Option<Container> {
    let node = match (container, seg) {
        (Container::Object(obj), JsonPathSegment::Key(k)) => obj.get(k)?.value()?,
        (Container::Array(arr), JsonPathSegment::Index(idx)) => arr.elements().get(*idx)?.clone(),
        _ => return None,
    };
    if need_array {
        node.as_array().map(Container::Array)
    } else {
        node.as_object().map(Container::Object)
    }
}

#[cfg(test)]
mod tests;
