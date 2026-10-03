// ---
// relationships:
//   implements: command-line-interface
// ---
//! The offline bundled demo template.
//!
//! `toha-demo` is a reserved fallback name. Registry resolution always wins; the
//! bundled demo answers only when every registry layer returns `NotFound` for
//! exactly this name. All embedding lives here, behind the `cli` feature: the
//! interview engine, `Template`, `Plan`, `apply`, `protocol`, and `staging`
//! learn nothing new, and `ResolvedTemplate` gains no field or variant.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use include_dir::{Dir, DirEntry, File, include_dir};
use sha2::{Digest, Sha256};
use toha::source;

use crate::Dirs;

use super::resolve::{ResolveError, ResolvedTemplate};

/// The one reserved resolution token and the demo's stable formal name.
/// Distinct from the demo template's own short name (`name: demo`).
pub const RESERVED: &str = "toha-demo";

/// The single maintained source, embedded at compile time. `include_dir!` reads
/// `docs/examples/demo/` directly, so the embedded copy IS the source — no drift.
static DEMO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo");

/// A read-only descriptor for the discoverability row (see D3). Not a registry
/// entry; never consulted by resolution.
pub struct BundledRow {
    pub formal: &'static str,
    pub short: &'static str,
    pub trusted: bool,
}

/// The read-only row the unfiltered `templates list` presents for the bundled
/// demo. Presentation only; resolution never consults it.
pub fn listing_row() -> BundledRow {
    BundledRow {
        formal: RESERVED,
        short: "demo",
        trusted: false,
    }
}

/// Content version of the embedded tree: lowercase sha256 hex (64 chars), derived
/// once at runtime from the embedded bytes. Changes iff the demo content changes;
/// a toha version bump that leaves the demo untouched keeps the same commit, so
/// staged interviews still resume. Satisfies the registry commit pattern
/// `^[0-9a-f]{40}([0-9a-f]{24})?$`.
pub fn commit() -> &'static str {
    static COMMIT: LazyLock<String> = LazyLock::new(|| digest(&DEMO));
    &COMMIT
}

/// Resolve the reserved demo on first run: materialize + describe.
/// Called only after every registry layer returned `NotFound` for exactly
/// `RESERVED`.
pub fn resolve(dirs: &Dirs) -> Result<ResolvedTemplate, ResolveError> {
    let folder = materialize(dirs)?;
    Ok(ResolvedTemplate {
        formal_name: RESERVED.to_string(),
        commit: commit().to_string(),
        folder,
        // The hookless demo is never trusted; it carries no approval digest.
        approval: None,
        named: false,
        aliases: Vec::new(),
        source: None,
    })
}

/// Reconstruct the reserved demo from a staged identity alone, offline. Returns
/// `None` when the record is not the bundled identity, so `resume_template` falls
/// through to its existing paths. `Some(Err(..))` with a clear "stage again"
/// message when the staged commit is not this build's commit.
pub fn resume(
    formal: &str,
    commit_arg: &str,
    dirs: &Dirs,
) -> Option<Result<ResolvedTemplate, ResolveError>> {
    if formal != RESERVED {
        return None;
    }
    if commit_arg != commit() {
        return Some(Err(ResolveError::text(format!(
            "the bundled {RESERVED} changed since this interview was staged; \
             stage {RESERVED} again"
        ))));
    }
    Some(resolve(dirs))
}

/// The content-addressed cache slot for the embedded tree, materialized once.
/// Fills a sibling tempdir with create-new writes, then atomically renames it
/// into `<commit>`; an existing slot is reused. Idempotent and race-safe across
/// processes. Writes only where toha already writes template sources.
fn materialize(dirs: &Dirs) -> Result<PathBuf, ResolveError> {
    let root = dirs
        .cache
        .join("sources")
        .join(source::install_key(RESERVED))
        .join(commit());
    if !root.is_dir() {
        let parent = root
            .parent()
            .ok_or_else(|| ResolveError::text("cache path has no parent"))?;
        fs::create_dir_all(parent).map_err(ResolveError::text)?;
        let temporary = tempfile::tempdir_in(parent).map_err(ResolveError::text)?;
        let staging = temporary.path().join("demo");
        write_tree(&DEMO, &staging)?;
        if !root.exists() {
            fs::rename(&staging, &root).map_err(ResolveError::text)?;
        }
    }
    Ok(root)
}

/// The embedded files in a stable root-relative order.
fn files(dir: &'static Dir<'static>) -> Vec<&'static File<'static>> {
    fn collect(dir: &'static Dir<'static>, out: &mut Vec<&'static File<'static>>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => collect(child, out),
                DirEntry::File(file) => out.push(file),
            }
        }
    }
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort_by_key(|file| relative(file));
    out
}

/// A file's root-relative path with `/` separators, so the digest and ordering
/// are identical on every platform.
fn relative(file: &File<'_>) -> String {
    file.path().to_string_lossy().replace('\\', "/")
}

/// Canonical digest: files sorted by root-relative path; for each file feed
/// `len(path)‖path‖len(bytes)‖bytes` (lengths as `u64` LE) into sha256.
fn digest(dir: &'static Dir<'static>) -> String {
    let mut hasher = Sha256::new();
    for file in files(dir) {
        let path = relative(file);
        let path = path.as_bytes();
        let bytes = file.contents();
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

/// Write the embedded tree to `destination` with create-new writes, so a second
/// writer racing into the same tempdir cannot clobber the first.
fn write_tree(dir: &'static Dir<'static>, destination: &Path) -> Result<(), ResolveError> {
    for file in files(dir) {
        let path = destination.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ResolveError::text)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(ResolveError::text)?;
        output
            .write_all(file.contents())
            .map_err(ResolveError::text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/examples/demo")
    }

    fn dirs(root: &Path) -> Dirs {
        Dirs {
            system_config: root.join("system-config"),
            user_config: root.join("user-config"),
            local_config_override: None,
            system_data: root.join("system-data"),
            user_data: root.join("user-data"),
            cache: root.join("cache"),
            state: root.join("state"),
            home: root.join("home"),
        }
    }

    /// Every source file, root-relative with `/` separators.
    fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn embed_equals_source() {
        // Materialize the embedded tree offline and prove it is byte-identical to
        // docs/examples/demo/ — the single maintained source.
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        let resolved = resolve(&dirs).unwrap();
        assert_eq!(resolved.formal_name, RESERVED);
        assert_eq!(resolved.commit, commit());
        assert!(resolved.approval.is_none());
        assert!(!resolved.named);
        assert!(resolved.folder.ends_with(commit()));
        assert_eq!(tree(&resolved.folder), tree(&source_root()));
    }

    #[test]
    fn resolve_is_idempotent_and_reuses_the_slot() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        let first = resolve(&dirs).unwrap().folder;
        let second = resolve(&dirs).unwrap().folder;
        assert_eq!(first, second);
        assert!(second.is_dir());
    }

    #[test]
    fn resume_reconstructs_the_bundled_identity_offline() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        let resolved = resume(RESERVED, commit(), &dirs)
            .expect("bundled identity")
            .expect("materializes offline");
        assert_eq!(resolved.formal_name, RESERVED);
        assert_eq!(resolved.commit, commit());
        assert_eq!(tree(&resolved.folder), tree(&source_root()));
    }

    #[test]
    fn resume_falls_through_for_other_formal_names() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        assert!(resume("gh:owner/repo", commit(), &dirs).is_none());
        assert!(resume("/tmp/some/folder", "", &dirs).is_none());
    }

    #[test]
    fn resume_guards_a_stale_commit() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        let stale = "0".repeat(64);
        let result = resume(RESERVED, &stale, &dirs).expect("bundled identity");
        match result {
            Err(ResolveError::Error(message)) => {
                assert!(message.contains("stage toha-demo again"), "{message}");
            }
            other => panic!("expected a stale-commit error, got {other:?}"),
        }
    }

    #[test]
    fn commit_is_lowercase_sha256_hex() {
        let commit = commit();
        assert_eq!(commit.len(), 64);
        assert!(
            commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }

    #[test]
    fn commit_derives_from_source() {
        // Recompute the digest straight from the on-disk source and compare it
        // to the runtime-derived commit. Any drift between the embed, the source,
        // and the identity fails here.
        let mut files: Vec<PathBuf> = Vec::new();
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.push(path);
                }
            }
        }
        let root = source_root();
        walk(&root, &mut files);
        let mut relative: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        relative.sort();
        let mut hasher = Sha256::new();
        for rel in &relative {
            let bytes = fs::read(root.join(rel)).unwrap();
            hasher.update((rel.len() as u64).to_le_bytes());
            hasher.update(rel.as_bytes());
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        assert_eq!(format!("{:x}", hasher.finalize()), commit());
    }

    #[test]
    fn listing_row_short_name_matches_source() {
        // The listing row's short name is a literal; keep it bound to the demo
        // template's own `name` so the two cannot drift.
        let manifest = fs::read_to_string(source_root().join("template.yml")).unwrap();
        assert!(
            manifest.lines().any(|line| line.trim() == "name: demo"),
            "demo template.yml must declare `name: demo`"
        );
        assert_eq!(listing_row().short, "demo");
        assert_eq!(listing_row().formal, RESERVED);
        assert!(!listing_row().trusted);
    }
}
