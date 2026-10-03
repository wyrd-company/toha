// ---
// relationships:
//   implements: architecture
// ---
//! The reviewable executable surface of a template and the single trust rule.
//!
//! Approval binds to the reviewed executable content, never to a template's
//! name. [`HookSurface::of`] collects a template's hook nodes and the bytes of
//! every in-tree file they execute; [`HookSurface::digest`] fingerprints that
//! surface; [`evaluate_trust`] is the only producer of trust from stored state.
//!
//! The module is pure and UI-free: its only I/O is reading the bytes of
//! executed files inside the template root.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::template::Template;

/// Where a hook node sits in the reviewable surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOrigin {
    /// The `index`-th hook node reached in interview order.
    Interview { index: usize },
    /// The `index`-th top-level `hooks` entry.
    TopLevel { index: usize },
}

/// One executed in-tree file: its template-relative path and content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDigest {
    /// Template-relative path, `/`-separated.
    pub rel: String,
    /// SHA-256 of the file's bytes.
    pub sha256: [u8; 32],
}

/// One reviewable hook node: its parsed value and the files it executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookView {
    /// Position of the node in the surface.
    pub origin: HookOrigin,
    /// The parsed, post-`!include`, pre-render hook node value. The whole
    /// value is fingerprinted, so any field — known or future — is covered.
    pub node: Value,
    /// Every in-tree file the node executes, in declaration order.
    pub scripts: Vec<ScriptDigest>,
}

/// The reviewable executable surface of a template, in a stable order:
/// interview hook nodes (interview order) then top-level hooks (list order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSurface {
    hooks: Vec<HookView>,
}

/// A fingerprint of a template's whole executable surface.
///
/// Constructed only by [`HookSurface::digest`] or [`ReviewDigest::parse`]
/// against `^sha256:[0-9a-f]{64}$`; it cannot be hand-forged from an arbitrary
/// string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewDigest(String);

/// The trust of a template's current content against stored approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The stored approval matches the live surface; hooks may run.
    Trusted,
    /// No stored approval, or it no longer matches; the surface needs review.
    NeedsReview,
}

/// The difference between a live surface and a prior approved one, for
/// presentation. Nodes are matched by [`HookOrigin`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookSurfaceDiff {
    /// Nodes present now but absent from the prior surface.
    pub added: Vec<HookView>,
    /// Nodes present in the prior surface but absent now.
    pub removed: Vec<HookView>,
    /// Nodes at the same origin whose value or scripts changed, as (prior, now).
    pub changed: Vec<(HookView, HookView)>,
}

/// A surface that cannot be read, so the template cannot be trusted.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    /// An executed in-tree file is missing or unreadable.
    #[error("{path}: {source}")]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// A declared `script` target resolves outside the template root.
    #[error("{0}: executed file escapes the template root")]
    Escape(PathBuf),
}

impl ReviewDigest {
    /// Parses a stored digest, enforcing `^sha256:[0-9a-f]{64}$`.
    pub fn parse(value: &str) -> Result<Self, String> {
        if is_digest(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(format!("invalid review digest: {value}"))
        }
    }
    /// The `sha256:<64 hex>` string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `sha256:` followed by exactly 64 lowercase hex digits.
fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

impl<'de> Deserialize<'de> for ReviewDigest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// The ONLY producer of trust from stored state. Equality of the stored
/// approval and the live surface digest is the whole policy.
pub fn evaluate_trust(approved: Option<&ReviewDigest>, live: &ReviewDigest) -> Trust {
    match approved {
        Some(approved) if approved == live => Trust::Trusted,
        _ => Trust::NeedsReview,
    }
}

/// The keys of a hook command whose value names an executed in-tree file whose
/// bytes are pulled into the digest. `script` names one declared file; `run`
/// and `args` hold token vectors whose literal in-tree entries are hashed.
const FILE_REFERENCING_KEYS: &[&str] = &["script", "run", "args"];
/// The keys of a hook command that are covered by the node value alone, with
/// no separate file bytes. Together with [`FILE_REFERENCING_KEYS`] this must
/// list every key the schema permits in a hook command; the coverage guard
/// test fails when the schema grows a key that is in neither set.
#[cfg(test)]
const NODE_ONLY_KEYS: &[&str] = &[
    "cwd",
    "each",
    "when",
    "id",
    "capture",
    "allow-failure",
    "parse",
    "status-id",
];

impl HookSurface {
    /// Collects the reviewable surface of `template`, reading the bytes of
    /// every in-tree file its hooks execute.
    pub fn of(template: &Template) -> Result<Self, ReviewError> {
        let mut hooks = Vec::new();
        for (index, node) in template.interview_hook_nodes.iter().enumerate() {
            let command = node.get("hook").unwrap_or(node);
            let scripts = executed_files(command, &template.root)?;
            hooks.push(HookView {
                origin: HookOrigin::Interview { index },
                node: node.clone(),
                scripts,
            });
        }
        for (index, node) in template.top_hook_nodes.iter().enumerate() {
            let scripts = executed_files(node, &template.root)?;
            hooks.push(HookView {
                origin: HookOrigin::TopLevel { index },
                node: node.clone(),
                scripts,
            });
        }
        Ok(Self { hooks })
    }

    /// The reviewable hook views, in surface order.
    pub fn views(&self) -> &[HookView] {
        &self.hooks
    }

    /// A deterministic, answer-independent fingerprint of the whole surface.
    pub fn digest(&self) -> ReviewDigest {
        let mut hasher = Sha256::new();
        hasher.update(b"toha-hook-surface-v1");
        frame_len(&mut hasher, self.hooks.len());
        for view in &self.hooks {
            let (tag, index) = match view.origin {
                HookOrigin::Interview { index } => (0u8, index),
                HookOrigin::TopLevel { index } => (1u8, index),
            };
            hasher.update([tag]);
            frame_len(&mut hasher, index);
            frame_bytes(&mut hasher, canonical_json(&view.node).as_bytes());
            frame_len(&mut hasher, view.scripts.len());
            for script in &view.scripts {
                frame_bytes(&mut hasher, script.rel.as_bytes());
                hasher.update(script.sha256);
            }
        }
        ReviewDigest(format!("sha256:{:x}", hasher.finalize()))
    }

    /// The difference between this live surface and a prior approved one.
    pub fn diff(&self, prior: &HookSurface) -> HookSurfaceDiff {
        let mut diff = HookSurfaceDiff::default();
        for view in &self.hooks {
            match prior.hooks.iter().find(|p| p.origin == view.origin) {
                None => diff.added.push(view.clone()),
                Some(before) if before != view => diff.changed.push((before.clone(), view.clone())),
                Some(_) => {}
            }
        }
        for before in &prior.hooks {
            if !self.hooks.iter().any(|v| v.origin == before.origin) {
                diff.removed.push(before.clone());
            }
        }
        diff
    }
}

/// The in-tree files a hook `command` object executes: its declared `script`
/// target, then every literal (no-Jinja) `run`/`args` token that resolves to a
/// file inside the template root. Jinja-substituted paths are not resolvable
/// before render and are left uncovered.
fn executed_files(command: &Value, root: &Path) -> Result<Vec<ScriptDigest>, ReviewError> {
    let mut scripts = Vec::new();
    for key in FILE_REFERENCING_KEYS {
        match *key {
            "script" => {
                if let Some(script) = command.get("script").and_then(Value::as_str) {
                    scripts.push(hash_declared(root, script)?);
                }
            }
            token_vector => {
                let tokens = command
                    .get(token_vector)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str);
                for token in tokens {
                    if is_literal(token)
                        && let Some(script) = hash_literal(root, token)?
                    {
                        scripts.push(script);
                    }
                }
            }
        }
    }
    Ok(scripts)
}

/// A token with no Jinja tags, so its rendered value equals the literal.
fn is_literal(token: &str) -> bool {
    !token.contains("{{") && !token.contains("{%") && !token.contains("{#")
}

/// Hashes a declared `script` target. It must resolve to a readable file
/// inside the template root; anything else means the surface cannot be read.
fn hash_declared(root: &Path, token: &str) -> Result<ScriptDigest, ReviewError> {
    if Path::new(token).is_absolute() {
        return Err(ReviewError::Escape(PathBuf::from(token)));
    }
    let candidate = root.join(token);
    let actual = candidate.canonicalize().map_err(|source| ReviewError::Io {
        path: candidate.clone(),
        source,
    })?;
    if !actual.starts_with(root) {
        return Err(ReviewError::Escape(actual));
    }
    hash_file(root, &actual).and_then(|script| {
        script.ok_or_else(|| ReviewError::Io {
            path: actual,
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a file"),
        })
    })
}

/// Hashes a literal `run`/`args` token when it resolves to a readable file
/// inside the template root, or yields `None` when it is an ordinary argument
/// or names a path outside the root (the accepted residual gap).
fn hash_literal(root: &Path, token: &str) -> Result<Option<ScriptDigest>, ReviewError> {
    if Path::new(token).is_absolute() {
        return Ok(None);
    }
    let candidate = root.join(token);
    let Ok(actual) = candidate.canonicalize() else {
        return Ok(None);
    };
    if !actual.starts_with(root) {
        return Ok(None);
    }
    hash_file(root, &actual)
}

/// Reads and hashes `actual` when it is a file, framing it with its
/// root-relative path. Yields `None` when it is not a regular file.
fn hash_file(root: &Path, actual: &Path) -> Result<Option<ScriptDigest>, ReviewError> {
    if !actual.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(actual).map_err(|source| ReviewError::Io {
        path: actual.to_owned(),
        source,
    })?;
    let rel = actual
        .strip_prefix(root)
        .unwrap_or(actual)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(Some(ScriptDigest {
        rel,
        sha256: Sha256::digest(&bytes).into(),
    }))
}

/// Length-prefixes a byte run so concatenated fields cannot be confused.
fn frame_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    frame_len(hasher, bytes.len());
    hasher.update(bytes);
}

/// Absorbs a length as 8 little-endian bytes.
fn frame_len(hasher: &mut Sha256, len: usize) {
    hasher.update((len as u64).to_le_bytes());
}

/// A deterministic JSON encoding with object keys sorted, independent of the
/// value's in-memory key order.
fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("string key"));
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&serde_json::to_string(scalar).expect("scalar value")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Writes `template.yml` and any support files, then loads the template.
    fn load(files: &[(&str, &str)]) -> Template {
        let dir = tempfile::tempdir().unwrap();
        for (rel, contents) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            #[cfg(unix)]
            if rel.ends_with(".sh") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        fs::create_dir_all(dir.path().join("template")).unwrap();
        let template = Template::load(dir.path()).unwrap();
        // Keep the directory alive for the test's lifetime by leaking it; the
        // OS reclaims it when the test process ends.
        std::mem::forget(dir);
        template
    }

    fn digest_of(files: &[(&str, &str)]) -> ReviewDigest {
        HookSurface::of(&load(files)).unwrap().digest()
    }

    #[test]
    fn digest_is_deterministic_and_valid() {
        let files = &[(
            "template.yml",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n",
        )];
        let first = digest_of(files);
        let second = digest_of(files);
        assert_eq!(first, second);
        assert!(is_digest(first.as_str()), "{}", first.as_str());
    }

    #[test]
    fn answer_and_documentation_changes_do_not_change_the_digest() {
        let with_hook = "name: sample\ninterview:\n  - id: who\n    type: text\n    prompt: who\nhooks:\n  - run: [ tool, call ]\n";
        // Extra questions, data, and messages leave the hook surface identical.
        let more_questions = "name: sample\ndata:\n  x: 1\ninterview:\n  - id: who\n    type: text\n    prompt: who\n  - id: what\n    type: text\n    prompt: what\nmessages:\n  after-apply: done\nhooks:\n  - run: [ tool, call ]\n";
        assert_eq!(
            digest_of(&[("template.yml", with_hook)]),
            digest_of(&[("template.yml", more_questions)]),
        );
    }

    #[test]
    fn every_hook_field_changes_the_digest() {
        // Each variant differs from the base by exactly one hook-node field,
        // read through `of()` from the retained parsed value.
        let base = "name: sample\nhooks:\n  - run: [ tool, call ]\n";
        let base_digest = digest_of(&[("template.yml", base)]);
        let variants = [
            "name: sample\nhooks:\n  - run: [ tool, other ]\n",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n    cwd: sub\n",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n    when: \"true\"\n",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n    each: \"[1] as item\"\n",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n  - run: [ tool, again ]\n",
        ];
        for variant in variants {
            assert_ne!(
                digest_of(&[("template.yml", variant)]),
                base_digest,
                "variant did not change the digest:\n{variant}"
            );
        }
    }

    #[test]
    fn canonical_encoding_is_sensitive_to_every_key() {
        // A lower-level guard on `canonical_json`: mutating any key of a node
        // value changes the encoding, independent of file bytes.
        let base = json!({"hook": {"run": ["tool", "call"], "cwd": "x"}});
        let surface = |node: &Value| {
            HookSurface {
                hooks: vec![HookView {
                    origin: HookOrigin::Interview { index: 0 },
                    node: node.clone(),
                    scripts: vec![],
                }],
            }
            .digest()
        };
        let base_digest = surface(&base);
        let mutations = [
            json!({"hook": {"run": ["tool", "other"], "cwd": "x"}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "y"}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "when": "true"}}),
            json!({"hook": {"run": ["tool", "call"]}}),
            json!({"when": "true", "hook": {"run": ["tool", "call"], "cwd": "x"}}),
            // A hook-result field is part of the reviewed node, so declaring one
            // changes the digest — a new stdio-affecting `capture`, or an `id`,
            // cannot slip past an existing approval unreviewed.
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "id": "t"}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "capture": ["stdout"]}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "allow-failure": true}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "parse": "json"}}),
            json!({"hook": {"run": ["tool", "call"], "cwd": "x", "status-id": "s"}}),
        ];
        for mutation in mutations {
            assert_ne!(surface(&mutation), base_digest, "{mutation}");
        }
    }

    #[test]
    fn declared_script_bytes_are_covered() {
        let template = "name: sample\nhooks:\n  - script: setup.sh\n";
        let before = digest_of(&[("template.yml", template), ("setup.sh", "echo one\n")]);
        let after = digest_of(&[("template.yml", template), ("setup.sh", "echo two\n")]);
        assert_ne!(before, after, "changed script bytes must change the digest");
    }

    #[test]
    fn literal_in_tree_run_path_bytes_are_covered() {
        let template = "name: sample\nhooks:\n  - run: [ python, scripts/x.py ]\n";
        let before = digest_of(&[("template.yml", template), ("scripts/x.py", "print(1)\n")]);
        let after = digest_of(&[("template.yml", template), ("scripts/x.py", "print(2)\n")]);
        assert_ne!(
            before, after,
            "changed run-target bytes must change the digest"
        );
    }

    #[test]
    fn jinja_run_path_bytes_are_the_accepted_residual_gap() {
        // A Jinja-substituted path cannot be resolved before render, so its
        // bytes are not covered; the digest does not change with them.
        let template = "name: sample\nhooks:\n  - run: [ python, \"scripts/{{ 'x' }}.py\" ]\n";
        let before = digest_of(&[("template.yml", template), ("scripts/x.py", "print(1)\n")]);
        let after = digest_of(&[("template.yml", template), ("scripts/x.py", "print(2)\n")]);
        assert_eq!(
            before, after,
            "a Jinja run target is out of the covered surface"
        );
    }

    #[test]
    fn a_missing_declared_script_cannot_be_read() {
        // The loader validates scripts, so exercise `of` directly on a template
        // whose script vanished after load.
        let template = load(&[
            (
                "template.yml",
                "name: sample\nhooks:\n  - script: setup.sh\n",
            ),
            ("setup.sh", "echo one\n"),
        ]);
        fs::remove_file(template.root.join("setup.sh")).unwrap();
        assert!(matches!(
            HookSurface::of(&template),
            Err(ReviewError::Io { .. })
        ));
    }

    #[test]
    fn parse_rejects_forged_digests() {
        assert!(
            ReviewDigest::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            )
            .is_ok()
        );
        for bad in [
            "trust me",
            "sha256:tooshort",
            "sha1:0000000000000000000000000000000000000000",
            "sha256:0000000000000000000000000000000000000000000000000000000000ABCDEF",
        ] {
            assert!(ReviewDigest::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn evaluate_trust_is_equality_of_stored_and_live() {
        let live = digest_of(&[("template.yml", "name: sample\nhooks:\n  - run: [ a ]\n")]);
        let other = digest_of(&[("template.yml", "name: sample\nhooks:\n  - run: [ b ]\n")]);
        assert_eq!(evaluate_trust(Some(&live), &live), Trust::Trusted);
        assert_eq!(evaluate_trust(Some(&other), &live), Trust::NeedsReview);
        assert_eq!(evaluate_trust(None, &live), Trust::NeedsReview);
    }

    #[test]
    fn diff_reports_added_removed_and_changed_nodes() {
        let prior = HookSurface::of(&load(&[(
            "template.yml",
            "name: sample\nhooks:\n  - run: [ tool, call ]\n  - run: [ tool, gone ]\n",
        )]))
        .unwrap();
        let live = HookSurface::of(&load(&[(
            "template.yml",
            "name: sample\nhooks:\n  - run: [ tool, changed ]\n",
        )]))
        .unwrap();
        let diff = live.diff(&prior);
        assert!(diff.added.is_empty());
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.changed.len(), 1);
    }

    /// The set of keys the schema permits in a hook command, read from the
    /// embedded template-format schema at both the top-level `hook-command`
    /// and the interview `hook-node`'s `hook` object.
    fn schema_hook_command_keys() -> std::collections::BTreeSet<String> {
        let schema: Value = serde_norway::from_str(include_str!(
            "../docs/specifications/template-format.schema.yml"
        ))
        .unwrap();
        let mut keys = std::collections::BTreeSet::new();
        let defs = &schema["$defs"];
        for object in [
            &defs["hook-command"],
            &defs["hook-node"]["properties"]["hook"],
        ] {
            for key in object["properties"].as_object().into_iter().flatten() {
                keys.insert(key.0.clone());
            }
        }
        keys
    }

    #[test]
    fn new_file_referencing_field_forces_a_coverage_decision() {
        // Every key the schema allows in a hook command must be classified as
        // file-referencing (its bytes are digested) or node-only (covered by
        // the node value alone). A new schema key in neither set fails here,
        // forcing a conscious coverage decision at the point it is introduced.
        let classified: std::collections::BTreeSet<String> = FILE_REFERENCING_KEYS
            .iter()
            .chain(NODE_ONLY_KEYS)
            .map(|k| (*k).to_owned())
            .collect();
        let schema_keys = schema_hook_command_keys();
        let unclassified: Vec<_> = schema_keys.difference(&classified).collect();
        assert!(
            unclassified.is_empty(),
            "hook command keys need a coverage decision: {unclassified:?}"
        );
        // The classification must not name keys the schema does not have.
        let extra: Vec<_> = classified.difference(&schema_keys).collect();
        assert!(
            extra.is_empty(),
            "classified keys absent from schema: {extra:?}"
        );
    }

    #[test]
    fn every_hook_key_is_classified_by_executable_reference() {
        // Every key the schema allows in a hook command must be classified as an
        // ExecutableReference — one that names an in-tree program (run, script) —
        // or a NoExecutableReference. This is the 1069 hook-review coverage
        // decision: no new field may name an executable without a conscious
        // choice here, so `run[0]`/`script` remain the only surfaces that derive a
        // program, and a result-reading field cannot smuggle one in. An
        // unclassified key fails; a classified key absent from the schema fails.
        const EXECUTABLE_REFERENCE: &[&str] = &["run", "script"];
        const NO_EXECUTABLE_REFERENCE: &[&str] = &[
            "args",
            "cwd",
            "when",
            "each",
            "id",
            "capture",
            "allow-failure",
            "parse",
            "status-id",
        ];
        let classified: std::collections::BTreeSet<String> = EXECUTABLE_REFERENCE
            .iter()
            .chain(NO_EXECUTABLE_REFERENCE)
            .map(|k| (*k).to_owned())
            .collect();
        let schema_keys = schema_hook_command_keys();
        let unclassified: Vec<_> = schema_keys.difference(&classified).collect();
        assert!(
            unclassified.is_empty(),
            "hook keys need an executable-reference decision (1069 coverage): {unclassified:?}"
        );
        let extra: Vec<_> = classified.difference(&schema_keys).collect();
        assert!(
            extra.is_empty(),
            "classified keys absent from schema: {extra:?}"
        );
    }
}
