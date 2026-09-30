// ---
// relationships:
//   informs: project-updates
// ---
//! gix spike: prove Toha's snapshot/update git operations run in-process via gitoxide.
//!
//! `cargo run --offline` (no args) runs the full scenario:
//!   * fixture setup and verification use the `git` binary (NOT under test),
//!   * every operation under test runs as `gix-spike p-* ...` in a child process,
//!     traced with `strace -f` when available, and the trace must contain no `execve`
//!     other than the child's own start.
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use gix::bstr::{BStr, BString, ByteSlice};
use gix::objs::tree::EntryKind;

type Res<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const TARGET: &str = "app"; // target directory, relative to the repo root
const SNAP_PREFIX: &str = "refs/toha/snapshots/";

fn main() -> Res {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None | Some("all") => all(),
        Some("p-status") => p_status(&args[2], &args[3]),
        Some("p-checkout") => p_checkout(&args[2], &args[3]),
        Some("p-snapshot") => p_snapshot(&args[2], &args[3], &args[4], &args[5]),
        Some("p-list") => p_list(&args[2]),
        Some("p-delete") => p_delete(&args[2], &args[3]),
        Some("p-merge") => p_merge(&args[2], &args[3], &args[4], &args[5]),
        Some(other) => Err(format!("unknown command {other}").into()),
    }
}

// ---------------------------------------------------------------------------
// Operations under test (no subprocesses)
// ---------------------------------------------------------------------------

/// Item 1: is `subdir` clean vs HEAD (staged, unstaged, untracked-not-ignored)?
fn p_status(repo: &str, subdir: &str) -> Res {
    let repo = gix::open(repo)?;
    let head_tree = repo.head_tree()?; // read HEAD's tree
    let _ = head_tree.id;
    let iter = repo
        .status(gix::progress::Discard)?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_rewrites(None)
        .into_iter([BString::from(format!(":(top){subdir}/"))])?;
    let mut dirty = Vec::new();
    for item in iter {
        let item = item?;
        let line = match &item {
            gix::status::Item::TreeIndex(c) => format!("staged {}", c.location()),
            gix::status::Item::IndexWorktree(i) => {
                format!("worktree {:?} {}", i.summary(), i.rela_path())
            }
        };
        dirty.push(line);
    }
    dirty.sort();
    if dirty.is_empty() {
        println!("STATUS {subdir}: CLEAN");
    } else {
        println!("STATUS {subdir}: DIRTY {dirty:?}");
    }
    Ok(())
}

/// Item 2: check out HEAD's full tree into `dest` (outside the repo, not a linked worktree).
fn p_checkout(repo: &str, dest: &str) -> Res {
    let repo = gix::open(repo)?;
    let tree_id = repo.head_tree_id()?;
    let mut index = repo.index_from_tree(&tree_id)?; // in-memory only, never written
    let mut opts = repo.checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)?;
    opts.destination_is_initially_empty = true;
    std::fs::create_dir_all(dest)?;
    let out = gix::worktree::state::checkout(
        &mut index,
        dest,
        repo.objects.clone().into_arc()?,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &AtomicBool::new(false),
        opts,
    )?;
    println!(
        "CHECKOUT {} files into {dest} (errors={}, collisions={})",
        out.files_updated,
        out.errors.len(),
        out.collisions.len()
    );
    Ok(())
}

/// Item 3/4: snapshot all files under `src_dir` into a parentless commit at refs/toha/snapshots/<id>.
/// `layout` = "a" (metadata JSON in commit message, tree = files) or
///            "b" (tree = { files/: <files>, snapshot.json }).
fn p_snapshot(repo: &str, src_dir: &str, id: &str, layout: &str) -> Res {
    let repo = gix::open(repo)?;
    let workdir = repo.workdir().ok_or("bare")?.to_owned();
    // Bonus: .gitignore check. Paths are checked as if they lived at <workdir>/app/<rel>.
    let index = repo.index_or_empty()?;
    let mut excludes = repo.excludes(
        &index,
        None,
        gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
    )?;

    let mut files = Vec::new();
    walk(Path::new(src_dir), Path::new(""), &mut files)?;
    let mut editor = repo.edit_tree(gix::ObjectId::empty_tree(repo.object_hash()))?;
    let mut skipped = Vec::new();
    for rel in files {
        let rel_s = rel.to_str().ok_or("non-utf8")?.replace('\\', "/");
        let in_repo = format!("{TARGET}/{rel_s}");
        let platform = excludes.at_path(Path::new(&in_repo), Some(gix::index::entry::Mode::FILE))?;
        if platform.is_excluded() {
            skipped.push(rel_s);
            continue;
        }
        let abs = Path::new(src_dir).join(&rel);
        let data = std::fs::read(&abs)?;
        let blob = repo.write_blob(&data)?;
        let kind = if is_executable(&abs)? { EntryKind::BlobExecutable } else { EntryKind::Blob };
        editor.upsert(rel_s.as_str(), kind, blob)?;
    }
    let files_tree = editor.write()?.detach();
    let meta = format!(
        "{{\"id\":\"{id}\",\"template\":\"example-template\",\"version\":\"{id}\",\"target\":\"{TARGET}\"}}"
    );
    let (commit_tree, message) = match layout {
        "a" => (files_tree, format!("toha snapshot {id}\n\n{meta}\n")),
        "b" => {
            let meta_blob = repo.write_blob(meta.as_bytes())?;
            let mut ed = repo.edit_tree(gix::ObjectId::empty_tree(repo.object_hash()))?;
            ed.upsert("files", EntryKind::Tree, files_tree)?;
            ed.upsert("snapshot.json", EntryKind::Blob, meta_blob)?;
            (ed.write()?.detach(), format!("toha snapshot {id}\n"))
        }
        _ => return Err("layout must be a|b".into()),
    };
    let sig = gix::actor::Signature {
        name: "Toha".into(),
        email: "toha@example.invalid".into(),
        time: gix::date::Time::now_local_or_utc(),
    };
    let mut tb1 = gix::date::parse::TimeBuf::default();
    let mut tb2 = gix::date::parse::TimeBuf::default();
    let refname = format!("{SNAP_PREFIX}{id}");
    // No parents => gix uses PreviousValue::MustNotExist for the ref: snapshots are immutable.
    let commit = repo.commit_as(
        sig.to_ref(&mut tb1),
        sig.to_ref(&mut tb2),
        refname.as_str(),
        message,
        commit_tree,
        Vec::<gix::ObjectId>::new(),
    )?;
    println!("SNAPSHOT {refname} -> {commit} (layout {layout}, gitignored-skipped={skipped:?})");
    let _ = workdir;
    Ok(())
}

fn p_list(repo: &str) -> Res {
    let repo = gix::open(repo)?;
    let platform = repo.references()?;
    for r in platform.prefixed(SNAP_PREFIX)? {
        let mut r = r?;
        let id = r.peel_to_id()?.detach();
        println!("REF {} {}", r.name().as_bstr(), id);
    }
    Ok(())
}

fn p_delete(repo: &str, id: &str) -> Res {
    let repo = gix::open(repo)?;
    repo.find_reference(format!("{SNAP_PREFIX}{id}").as_str())?.delete()?;
    println!("DELETED {SNAP_PREFIX}{id}");
    Ok(())
}

/// Resolve a snapshot's *files* tree, according to layout.
fn snapshot_files_tree(repo: &gix::Repository, id: &str, layout: &str) -> Res<gix::ObjectId> {
    let commit = repo
        .find_reference(format!("{SNAP_PREFIX}{id}").as_str())?
        .peel_to_commit()?;
    let tree = commit.tree()?;
    Ok(match layout {
        "a" => {
            let msg = commit.message_raw()?;
            let json = msg.lines().skip(2).next().unwrap_or_default();
            println!("META(a) {id}: {}", json.as_bstr());
            tree.id
        }
        _ => {
            let meta = tree.lookup_entry_by_path("snapshot.json")?.ok_or("no snapshot.json")?;
            println!("META(b) {id}: {}", meta.object()?.data.as_bstr());
            match tree.lookup_entry_by_path("files")? {
                Some(e) => e.object_id(),
                None => gix::ObjectId::empty_tree(repo.object_hash()), // empty template edge case
            }
        }
    })
}

/// Items 5+6: 3-way merge base=<old snapshot> ours=<new snapshot> theirs=HEAD:<TARGET>,
/// then apply to worktree + index with conflict stages.
fn p_merge(repo_path: &str, base_id: &str, ours_id: &str, layout: &str) -> Res {
    let repo = gix::open(repo_path)?;
    let workdir = repo.workdir().ok_or("bare")?.to_owned();
    let base_files = snapshot_files_tree(&repo, base_id, layout)?;
    let ours_files = snapshot_files_tree(&repo, ours_id, layout)?;

    // Graft snapshots into HEAD's tree at TARGET so the merge produces repo-root paths.
    // Everything outside TARGET is identical in all three trees, so it cannot conflict,
    // and user-only files inside TARGET appear as "added by theirs" (kept).
    let head_tree = repo.head_tree()?;
    let theirs_full = head_tree.id;
    let graft = |sub: gix::ObjectId| -> Res<gix::ObjectId> {
        let mut ed = repo.edit_tree(theirs_full)?;
        ed.upsert(TARGET, EntryKind::Tree, sub)?;
        Ok(ed.write()?.detach())
    };
    let base_full = graft(base_files)?;
    let ours_full = graft(ours_files)?;

    // Options: take repo config, then force diff3 markers and disable rename tracking.
    let mut opts: gix::merge::plumbing::tree::Options = repo.tree_merge_options()?.into();
    opts.rewrites = None; // rename detection OFF
    opts.blob_merge.text.conflict = gix::merge::blob::builtin_driver::text::Conflict::Keep {
        style: gix::merge::blob::builtin_driver::text::ConflictStyle::Diff3,
        marker_size: std::num::NonZeroU8::new(7).unwrap(),
    };
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: Some("toha snapshot (old template)".into()),
        current: Some("toha template (new)".into()),
        other: Some("your changes".into()),
    };
    let mut outcome = repo.merge_trees(base_full, ours_full, theirs_full, labels, opts.into())?;
    let how = gix::merge::tree::TreatAsUnresolved::git();
    for c in &outcome.conflicts {
        println!(
            "CONFLICT {} unresolved={} resolution={:?}",
            c.ours.location(),
            c.is_unresolved(how),
            c.resolution.as_ref().map(|_| "resolved").map_err(|e| format!("{e:?}"))
        );
    }
    let merged_full = outcome.tree.write()?.detach();

    // --- Apply to the worktree: only paths under TARGET that differ from HEAD:TARGET. ---
    let theirs_sub = subtree(&repo, theirs_full, TARGET)?;
    let merged_sub = subtree(&repo, merged_full, TARGET)?;
    let theirs_entries = flat_entries(&repo, theirs_sub)?;
    let merged_entries = flat_entries(&repo, merged_sub)?;
    for path in theirs_entries.keys() {
        if !merged_entries.contains_key(path) {
            let p = workdir.join(TARGET).join(gix::path::from_bstr(path.as_bstr()));
            std::fs::remove_file(&p)?;
            println!("WORKTREE delete {TARGET}/{path}");
        }
    }
    // Write adds/modifications via gix's checkout (filters, exec bit, stat info) on a tiny index.
    let mut write_state = gix::index::State::new(repo.object_hash());
    for (path, (id, mode)) in &merged_entries {
        if theirs_entries.get(path) != Some(&(*id, *mode)) {
            let full: BString = format!("{TARGET}/{path}").into();
            write_state.dangerously_push_entry(Default::default(), *id, gix::index::entry::Flags::empty(), *mode, full.as_bstr());
            println!("WORKTREE write {TARGET}/{path}");
        }
    }
    write_state.sort_entries();
    let mut copts = repo.checkout_options(gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping)?;
    copts.overwrite_existing = true;
    copts.destination_is_initially_empty = false;
    let co = gix::worktree::state::checkout(
        &mut write_state,
        &workdir,
        repo.objects.clone().into_arc()?,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &AtomicBool::new(false),
        copts,
    )?;
    if !co.errors.is_empty() || !co.collisions.is_empty() {
        return Err(format!("checkout errors {:?} collisions {:?}", co.errors, co.collisions).into());
    }

    // --- Index: keep everything outside TARGET as-is (user may have staged work elsewhere),
    //     replace TARGET/* by the merged tree's entries, then add conflict stages 1/2/3. ---
    let mut index = repo.open_index()?;
    let prefix = format!("{TARGET}/");
    index.remove_entries(|_, path, _| path.starts_with(prefix.as_bytes()));
    for (path, (id, mode)) in &merged_entries {
        let full: BString = format!("{TARGET}/{path}").into();
        let stat = write_state
            .entry_by_path(full.as_bstr())
            .map(|e| e.stat)
            .or_else(|| {
                // unchanged file: stat it so git doesn't need to rehash
                let md = gix::index::fs::Metadata::from_path_no_follow(&workdir.join(gix::path::from_bstr(full.as_bstr()))).ok()?;
                gix::index::entry::Stat::from_fs(&md).ok()
            })
            .unwrap_or_default();
        index.dangerously_push_entry(stat, *id, gix::index::entry::Flags::empty(), *mode, full.as_bstr());
    }
    index.sort_entries();
    index.remove_tree(); // cached TREE extension is stale now; git would commit a wrong tree otherwise
    index.remove_resolve_undo();
    let changed = outcome.index_changed_after_applying_conflicts(
        &mut index,
        how,
        gix::merge::tree::apply_index_entries::RemovalMode::Prune,
    );
    index.write(Default::default())?;
    println!("INDEX written (conflict stages added: {changed})");
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn subtree(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Res<gix::ObjectId> {
    let t = repo.find_tree(tree)?;
    Ok(match t.lookup_entry_by_path(path)? {
        Some(e) => e.object_id(),
        None => gix::ObjectId::empty_tree(repo.object_hash()),
    })
}

fn flat_entries(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> Res<BTreeMap<BString, (gix::ObjectId, gix::index::entry::Mode)>> {
    let st = gix::index::State::from_tree(&tree, &repo.objects, Default::default())?;
    Ok(st
        .entries()
        .iter()
        .map(|e| (e.path(&st).to_owned(), (e.id, e.mode)))
        .collect())
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Res {
    for entry in std::fs::read_dir(root.join(rel))? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let r = rel.join(&name);
        if entry.file_type()?.is_dir() {
            walk(root, &r, out)?;
        } else {
            out.push(r);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(p: &Path) -> Res<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(p)?.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn is_executable(_: &Path) -> Res<bool> {
    Ok(false)
}

// ---------------------------------------------------------------------------
// Orchestration: fixtures + verification (git binary allowed here only)
// ---------------------------------------------------------------------------

fn git(repo: &Path, args: &[&str]) -> Res<(bool, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), s))
}

fn gitok(repo: &Path, args: &[&str]) -> Res<String> {
    let (ok, s) = git(repo, args)?;
    if !ok {
        return Err(format!("git {args:?} failed: {s}").into());
    }
    Ok(s)
}

fn write(p: &Path, content: &[u8]) -> Res {
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, content)?;
    Ok(())
}

/// Run an operation under test in a child process of this binary, traced by strace if present.
/// Fails if the trace shows any execve besides the child's own.
fn prove(args: &[&str]) -> Res<String> {
    let exe = std::env::current_exe()?;
    let have_strace = Command::new("strace").arg("-V").output().is_ok();
    let trace = std::env::temp_dir().join(format!("gix-spike-trace-{}", std::process::id()));
    let out = if have_strace {
        Command::new("strace")
            .args(["-f", "-qq", "-e", "trace=execve,execveat,fork,vfork,clone,clone3", "-o"])
            .arg(&trace)
            .arg(&exe)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()?
    } else {
        Command::new(&exe).args(args).output()?
    };
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(format!("{args:?} failed: {stdout}{}", String::from_utf8_lossy(&out.stderr)).into());
    }
    if have_strace {
        let t = std::fs::read_to_string(&trace)?;
        let _ = std::fs::remove_file(&trace);
        let execs: Vec<_> = t.lines().filter(|l| l.contains("execve")).collect();
        let procs: Vec<_> = t
            .lines()
            .filter(|l| (l.contains("clone") && !l.contains("CLONE_THREAD")) || l.contains("fork("))
            .filter(|l| !l.contains("resumed")) // continuation lines
            .collect();
        let threads = t.lines().filter(|l| l.contains("CLONE_THREAD")).count();
        if execs.len() != 1 || !procs.is_empty() {
            return Err(format!("SUBPROCESS DETECTED for {args:?}:\n{}", t).into());
        }
        print!("  [strace: 1 execve (self), {threads} thread clones, 0 process clones] ");
    } else {
        print!("  [strace unavailable: UNVERIFIED] ");
    }
    println!("{}", args[0]);
    for l in stdout.lines() {
        println!("    {l}");
    }
    Ok(stdout)
}

fn check(cond: bool, what: &str) -> Res {
    println!("  {} {what}", if cond { "PASS" } else { "FAIL" });
    if cond { Ok(()) } else { Err(format!("check failed: {what}").into()) }
}

fn all() -> Res {
    let root = std::env::current_dir()?.join("work");
    let _ = std::fs::remove_dir_all(&root);
    for layout in ["a", "b"] {
        println!("\n=========== layout {layout} ===========");
        scenario(&root.join(layout), layout)?;
    }
    println!("\nALL CHECKS PASSED");
    Ok(())
}

const BIN_V1: &[u8] = b"\x89BIN\x00\x01\x02\x03v1\x00";

fn scenario(root: &Path, layout: &str) -> Res {
    let repo = root.join("repo");
    let rs = repo.to_str().unwrap();
    std::fs::create_dir_all(&repo)?;
    gitok(&repo, &["init", "-q", "-b", "main"])?;

    // ---- toha apply v1 writes template files into app/ ----
    let app = repo.join(TARGET);
    let lines5 = "one\ntwo\nthree\nfour\nfive\n";
    write(&app.join("t_only.txt"), lines5.as_bytes())?;
    write(&app.join("u_only.txt"), lines5.as_bytes())?;
    write(&app.join("both.txt"), lines5.as_bytes())?;
    write(&app.join("removed_clean.txt"), b"bye\n")?;
    write(&app.join("removed_edited.txt"), b"bye\n")?;
    write(&app.join("sub/nested.txt"), b"nested v1\n")?;
    write(&app.join("logo.bin"), BIN_V1)?;
    write(&app.join("debug.log"), b"ignored by .gitignore\n")?;
    write(&repo.join(".gitignore"), b"*.log\n")?;
    write(&repo.join("README.md"), b"user readme\n")?;

    println!("-- item 3: snapshot v1 from files on disk (repo/app)");
    prove(&["p-snapshot", rs, app.to_str().unwrap(), "v1", layout])?;
    gitok(&repo, &["add", "-A"])?;
    gitok(&repo, &["commit", "-q", "-m", "apply template v1"])?;

    // ---- user work, committed ----
    write(&app.join("u_only.txt"), b"one\ntwo\nTHREE-by-user\nfour\nfive\n")?;
    write(&app.join("both.txt"), b"one\ntwo\nthree-USER\nfour\nfive\n")?;
    write(&app.join("removed_edited.txt"), b"bye\nuser edit\n")?;
    write(&app.join("logo.bin"), b"\x89BIN\x00\x01\x02\x03user\x00")?;
    write(&app.join("user_notes.md"), b"user file in target dir\n")?;
    write(&repo.join("lib/other.rs"), b"// unrelated user file\n")?;
    gitok(&repo, &["add", "-A"])?;
    gitok(&repo, &["commit", "-q", "-m", "user edits"])?;

    println!("-- item 1: target dir cleanliness");
    let s = prove(&["p-status", rs, TARGET])?;
    check(s.contains("CLEAN"), "clean target reported CLEAN (ignored debug.log not counted)")?;
    write(&app.join("u_only.txt"), b"dirty\n")?;
    let s = prove(&["p-status", rs, TARGET])?;
    check(s.contains("DIRTY") && s.contains("u_only.txt"), "unstaged modification detected")?;
    gitok(&repo, &["add", "app/u_only.txt"])?;
    let s = prove(&["p-status", rs, TARGET])?;
    check(s.contains("staged app/u_only.txt"), "staged modification detected")?;
    gitok(&repo, &["reset", "-q", "--hard"])?;
    write(&app.join("new_untracked.txt"), b"x\n")?;
    write(&repo.join("elsewhere.txt"), b"x\n")?;
    let s = prove(&["p-status", rs, TARGET])?;
    check(
        s.contains("new_untracked.txt") && !s.contains("elsewhere"),
        "untracked file in target detected; outside target ignored",
    )?;
    std::fs::remove_file(app.join("new_untracked.txt"))?;
    let s = prove(&["p-status", rs, TARGET])?;
    check(s.contains("CLEAN"), "clean again (dirt outside target does not count)")?;
    std::fs::remove_file(repo.join("elsewhere.txt"))?;

    println!("-- item 2: checkout HEAD into a temp dir outside the repo");
    let scratch = root.join("scratch-checkout");
    prove(&["p-checkout", rs, scratch.to_str().unwrap()])?;
    check(scratch.join("lib/other.rs").exists() && scratch.join("app/both.txt").exists(), "files checked out")?;
    check(!scratch.join(".git").exists(), "no .git in scratch dir")?;
    let wt = gitok(&repo, &["worktree", "list"])?;
    check(wt.lines().count() == 1, "no linked worktree registered")?;

    // ---- render template v2 into the scratch checkout (Toha would do this) ----
    let sapp = scratch.join(TARGET);
    std::fs::remove_dir_all(&sapp)?; // template output only
    write(&sapp.join("t_only.txt"), b"one\ntwo\nthree-TEMPLATE\nfour\nfive\n")?;
    write(&sapp.join("u_only.txt"), lines5.as_bytes())?;
    write(&sapp.join("both.txt"), b"one\ntwo\nthree-TEMPLATE\nfour\nfive\n")?;
    write(&sapp.join("sub/nested.txt"), b"nested v1\n")?;
    write(&sapp.join("added.txt"), b"new in v2\n")?;
    write(&sapp.join("logo.bin"), b"\x89BIN\x00\x01\x02\x03v2\x00")?;

    println!("-- item 3: snapshot v2 from scratch dir, list, delete");
    prove(&["p-snapshot", rs, sapp.to_str().unwrap(), "v2", layout])?;
    // a throwaway snapshot to delete
    prove(&["p-snapshot", rs, sapp.to_str().unwrap(), "tmp", layout])?;
    let l = prove(&["p-list", rs])?;
    check(l.lines().count() == 3, "3 snapshot refs listed")?;
    prove(&["p-delete", rs, "tmp"])?;
    let l = prove(&["p-list", rs])?;
    check(l.lines().count() == 2 && !l.contains("tmp"), "tmp deleted")?;
    let parents = gitok(&repo, &["rev-list", "--parents", "-n1", "refs/toha/snapshots/v2"])?;
    check(parents.split_whitespace().count() == 1, "snapshot commit is parentless (git rev-list)")?;
    let ls = gitok(&repo, &["ls-tree", "-r", "--name-only", "refs/toha/snapshots/v1"])?;
    println!("    git ls-tree v1: {:?}", ls.lines().collect::<Vec<_>>());
    check(!ls.contains("debug.log"), "gitignored debug.log excluded from snapshot")?;
    gitok(&repo, &["fsck", "--no-progress"])?;
    let log = gitok(&repo, &["log", "--oneline", "main"])?;
    check(log.lines().count() == 2, "user branch untouched by snapshots")?;

    println!("-- items 4-6: merge v1 -> v2 into working tree + index");
    prove(&["p-merge", rs, "v1", "v2", layout])?;

    println!("-- verification with real git");
    let st = gitok(&repo, &["status", "--porcelain"])?;
    println!("    git status --porcelain:\n{}", indent(&st));
    let unmerged = gitok(&repo, &["ls-files", "-u"])?;
    println!("    git ls-files -u:\n{}", indent(&unmerged));
    let rd = |p: &str| std::fs::read(app.join(p)).unwrap_or_default();
    check(rd("t_only.txt") == b"one\ntwo\nthree-TEMPLATE\nfour\nfive\n", "(i) template-only change applied")?;
    check(rd("u_only.txt") == b"one\ntwo\nTHREE-by-user\nfour\nfive\n", "(ii) user-only change kept")?;
    let both = String::from_utf8(rd("both.txt"))?;
    println!("    both.txt:\n{}", indent(&both));
    check(
        both.contains("<<<<<<< toha template (new)")
            && both.contains("||||||| toha snapshot (old template)")
            && both.contains(">>>>>>> your changes"),
        "(iii) diff3 conflict markers with custom labels",
    )?;
    check(st.contains("UU app/both.txt"), "(iii) git status: both modified")?;
    check(!app.join("removed_clean.txt").exists() && st.contains("D  app/removed_clean.txt"), "(iv) removed by template, unedited -> deleted (staged deletion)")?;
    check(
        app.join("removed_edited.txt").exists() && st.contains("DU app/removed_edited.txt"),
        "(v) removed by template, edited by user -> modify/delete conflict (DU = deleted by us/template)",
    )?;
    check(rd("added.txt") == b"new in v2\n" && st.contains("A  app/added.txt"), "(vi) new template file added")?;
    check(
        rd("user_notes.md") == b"user file in target dir\n" && repo.join("lib/other.rs").exists() && !st.contains("user_notes") && !st.contains("other.rs"),
        "(vii) unrelated user files untouched",
    )?;
    check(st.contains("UU app/logo.bin"), "(viii) binary changed on both sides -> conflict")?;
    check(unmerged.lines().any(|l| l.contains(" 1\tapp/logo.bin")) && unmerged.lines().any(|l| l.contains(" 3\tapp/logo.bin")), "stages 1/2/3 present")?;
    let long = gitok(&repo, &["status"])?;
    let long_lines: Vec<_> = long.lines().filter(|l| l.contains("both modified") || l.contains("deleted by")).collect();
    println!("    git status (long) conflict lines: {long_lines:?}");
    check(long.contains("both modified:   app/both.txt") && long.contains("deleted by us:   app/removed_edited.txt"), "git status shows 'both modified' / 'deleted by us'")?;
    let (ok, out) = git(&repo, &["commit", "-m", "should fail"])?;
    println!("    git commit -m => ok={ok}: {}", out.lines().next().unwrap_or(""));
    check(!ok && out.contains("unmerged files"), "git commit refuses: 'Committing is not possible because you have unmerged files'")?;
    let (ok2, _) = git(&repo, &["write-tree"])?;
    check(!ok2, "git write-tree refuses (index has unmerged entries)")?;
    let log = gitok(&repo, &["log", "--oneline", "main"])?;
    check(log.lines().count() == 2, "no commit made on user branch")?;
    // Git itself lets `commit -a` through: `add -u` resolves unmerged entries (same after a real `git merge`).
    let (ok, out) = git(&repo, &["commit", "-am", "commit -a"])?;
    println!("    NOTE git commit -am => ok={ok}: {} (git behaves the same after a real `git merge` conflict)", out.lines().next().unwrap_or(""));
    Ok(())
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("      {l}")).collect::<Vec<_>>().join("\n")
}

#[allow(dead_code)]
fn _unused(_: &BStr) {}
