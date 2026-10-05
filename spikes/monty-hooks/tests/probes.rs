//! Probes for the three claims the spike must prove or falsify, plus the
//! findings that shape a design. Each test prints what it observed so the
//! run log is the evidence.

use std::{fs, path::PathBuf};

use monty_hooks_spike::{HookOptions, HookRun, Mode, run_hook};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    target: PathBuf,
}

const SECRET: &str = "outside the target\n";

/// `root/secret.txt`, `root/outside/`, and `root/target/` holding a small
/// project with a `.git` directory.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let target = root.join("target");
    fs::create_dir_all(target.join(".git/hooks")).unwrap();
    fs::create_dir_all(target.join("src")).unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    fs::write(root.join("secret.txt"), SECRET).unwrap();
    fs::write(target.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    fs::write(target.join("README.md"), "# NAME\n\nVersion: 0.0.0\n").unwrap();
    fs::write(target.join("settings.json"), "{\"name\": \"widget\", \"features\": []}\n").unwrap();
    fs::write(target.join("src/main.txt"), "alpha\nbeta\n").unwrap();
    Fixture { _dir: dir, root, target }
}

fn show(label: &str, run: &HookRun) {
    println!(
        "[{label}] result={:?} printed={:?} spawned={:?} unmounted={:?} elapsed={:?}",
        run.result.as_ref().map(|v| v.py_repr()).map_err(|e| e.to_string()),
        run.printed,
        run.spawned,
        run.unmounted_os_calls,
        run.elapsed
    );
}

fn assert_outside_untouched(f: &Fixture) {
    assert_eq!(fs::read_to_string(f.root.join("secret.txt")).unwrap(), SECRET);
    assert_eq!(fs::read_dir(f.root.join("outside")).unwrap().count(), 0);
    let mut names: Vec<_> = fs::read_dir(&f.root).unwrap().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    assert_eq!(names, ["outside", "secret.txt", "target"]);
}

// ---------------------------------------------------------------------------
// Claim 1: a hook edits a file inside the target mount.
// ---------------------------------------------------------------------------

#[test]
fn edits_files_inside_target() {
    let f = fixture();
    let code = r#"
import json, re
from pathlib import Path

readme = Path('README.md')
text = readme.read_text()
text = re.sub(r'^# NAME$', '# widget', text, flags=re.M)
text = text.replace('0.0.0', '1.2.3')
readme.write_text(text)

settings = Path('/target/settings.json')
data = json.loads(settings.read_text())
data['features'].append('logging')
settings.write_text(json.dumps(data, indent=2) + '\n')

Path('src/generated').mkdir(parents=True, exist_ok=True)
lines = Path('src/main.txt').read_text().splitlines()
Path('src/generated/upper.txt').write_text('\n'.join(l.upper() for l in lines) + '\n')

with open('src/main.txt', 'a') as fh:
    fh.write('gamma\n')

sorted(str(p) for p in Path('/target/src').iterdir())
"#;
    let run = run_hook(code, &f.target, HookOptions::default());
    show("edit", &run);
    assert!(run.result.is_ok());
    assert_eq!(fs::read_to_string(f.target.join("README.md")).unwrap(), "# widget\n\nVersion: 1.2.3\n");
    assert_eq!(
        fs::read_to_string(f.target.join("settings.json")).unwrap(),
        "{\n  \"name\": \"widget\",\n  \"features\": [\n    \"logging\"\n  ]\n}\n"
    );
    assert_eq!(fs::read_to_string(f.target.join("src/generated/upper.txt")).unwrap(), "ALPHA\nBETA\n");
    assert_eq!(fs::read_to_string(f.target.join("src/main.txt")).unwrap(), "alpha\nbeta\ngamma\n");
    assert!(run.spawned.is_empty());
    assert_outside_untouched(&f);
}

// ---------------------------------------------------------------------------
// Claim 2: a hook cannot read or write outside the target.
// ---------------------------------------------------------------------------

/// Each probe must raise; none may return host data or write a file.
const ESCAPES: &[(&str, &str)] = &[
    ("abs-read", "open('/etc/passwd').read()"),
    ("abs-read-pathlib", "from pathlib import Path\nPath('/etc/hostname').read_text()"),
    ("dotdot-read", "open('../secret.txt').read()"),
    ("dotdot-abs-read", "open('/target/../secret.txt').read()"),
    ("deep-dotdot-read", "open('/target/src/../../secret.txt').read()"),
    ("proc-read", "open('/proc/self/environ').read()"),
    ("abs-write", "open('/tmp/monty-spike-escape', 'w').write('x')"),
    ("dotdot-write", "open('../outside/new.txt', 'w').write('x')"),
    ("rename-out", "import os\nos.rename('README.md', '../outside/README.md')"),
    ("pathlib-rename-out", "from pathlib import Path\nPath('README.md').rename('/outside/README.md')"),
    ("mkdir-out", "from pathlib import Path\nPath('/outside/made').mkdir()"),
    ("unlink-out", "from pathlib import Path\nPath('../secret.txt').unlink()"),
    ("listdir-root", "import os\nos.listdir('/')"),
    ("chdir-out", "import os\nos.chdir('/')\nopen('etc/passwd').read()"),
    ("symlink-rel-read", "open('link-rel').read()"),
    ("symlink-abs-read", "open('link-abs').read()"),
    ("symlink-dir-read", "open('link-dir/secret.txt').read()"),
    ("symlink-rel-write", "open('link-rel', 'w').write('pwned')"),
    ("symlink-dir-write", "open('link-outdir/new.txt', 'w').write('pwned')"),
    ("env-read", "import os\nos.getenv('HOME')"),
    ("environ-read", "import os\nos.environ['PATH']"),
];

#[test]
fn cannot_read_or_write_outside_target() {
    let f = fixture();
    // Symlinks the operator already has inside the project, pointing out.
    std::os::unix::fs::symlink("../secret.txt", f.target.join("link-rel")).unwrap();
    std::os::unix::fs::symlink(f.root.join("secret.txt"), f.target.join("link-abs")).unwrap();
    std::os::unix::fs::symlink("..", f.target.join("link-dir")).unwrap();
    std::os::unix::fs::symlink("../outside", f.target.join("link-outdir")).unwrap();
    let _ = fs::remove_file("/tmp/monty-spike-escape");

    let mut failures = Vec::new();
    for (label, code) in ESCAPES {
        let run = run_hook(code, &f.target, HookOptions::default());
        show(label, &run);
        let leaked = run.result.as_ref().is_ok_and(|v| v.py_repr().contains("outside the target"));
        let env_value = run.result.as_ref().is_ok_and(|v| *v != monty_types::MontyObject::none());
        if leaked || env_value {
            failures.push(*label);
        }
    }
    assert!(failures.is_empty(), "escapes: {failures:?}");
    assert!(!PathBuf::from("/tmp/monty-spike-escape").exists());
    assert_eq!(fs::read_to_string(f.root.join("secret.txt")).unwrap(), SECRET);
    assert_eq!(fs::read_dir(f.root.join("outside")).unwrap().count(), 0);
    assert!(f.target.join("README.md").exists());
}

#[test]
fn read_only_mount_refuses_writes() {
    let f = fixture();
    let options = HookOptions { mode: Mode::ReadOnly, ..HookOptions::default() };
    let run = run_hook("open('README.md', 'w').write('x')", &f.target, options);
    show("ro-write", &run);
    assert_eq!(run.error_type().as_deref(), Some("PermissionError"));
    assert_eq!(fs::read_to_string(f.target.join("README.md")).unwrap(), "# NAME\n\nVersion: 0.0.0\n");
}

// ---------------------------------------------------------------------------
// Claim 3: a hook cannot run a process except through the host function.
// ---------------------------------------------------------------------------

const PROCESS_ATTEMPTS: &[(&str, &str)] = &[
    ("import-subprocess", "import subprocess\nsubprocess.run(['true'])"),
    ("os-system", "import os\nos.system('true')"),
    ("os-popen", "import os\nos.popen('true')"),
    ("os-exec", "import os\nos.execv('/bin/true', ['true'])"),
    ("os-fork", "import os\nos.fork()"),
    ("dunder-import", "__import__('subprocess')"),
    ("importlib", "import importlib\nimportlib.import_module('subprocess')"),
    ("exec-import", "exec('import subprocess')"),
    ("eval-import", "eval(\"__import__('os').system('true')\")"),
    ("builtins", "import builtins\nbuiltins.__import__('subprocess')"),
    ("ctypes", "import ctypes"),
    ("sys-modules", "import sys\nsys.modules['os'].system('true')"),
    ("class-walk", "().__class__.__base__.__subclasses__()"),
    ("socket", "import socket"),
    ("threading", "import threading"),
    ("asyncio-subprocess", "import asyncio\nasyncio.create_subprocess_exec('true')"),
    ("undeclared-host-fn", "spawn(['true'])"),
];

#[test]
fn cannot_spawn_without_host_function() {
    let f = fixture();
    let mut ran = Vec::new();
    for (label, code) in PROCESS_ATTEMPTS {
        let run = run_hook(code, &f.target, HookOptions { trusted: true, ..HookOptions::default() });
        show(label, &run);
        // The only spawn path is `run`; none of these call it.
        assert!(run.spawned.is_empty(), "{label}");
        if run.result.is_ok() {
            ran.push(*label);
        }
    }
    // `class-walk` may evaluate without reaching anything; every other probe must raise.
    ran.retain(|l| *l != "class-walk");
    assert!(ran.is_empty(), "completed without raising: {ran:?}");
}

#[test]
fn host_run_refused_when_untrusted_and_uncatchable() {
    let f = fixture();
    let code = r#"
try:
    run(['touch', 'made-by-run'])
except Exception as e:
    print('caught', type(e).__name__)
'continued'
"#;
    let run = run_hook(code, &f.target, HookOptions::default());
    show("untrusted-run", &run);
    assert_eq!(run.error_type().as_deref(), Some("PermissionError"));
    assert!(run.printed.is_empty(), "the hook must not catch the refusal");
    assert!(run.spawned.is_empty());
    assert!(!f.target.join("made-by-run").exists());
}

#[test]
fn host_run_spawns_when_trusted() {
    let f = fixture();
    let code = r#"
r = run(['git', 'rev-parse', '--is-inside-work-tree'])
s = run(['sh', '-c', 'pwd; echo err >&2; exit 3'])
(r['exit_code'], s['exit_code'], s['stdout'].strip().endswith('/target'), s['stderr'])
"#;
    let run = run_hook(code, &f.target, HookOptions { trusted: true, ..HookOptions::default() });
    show("trusted-run", &run);
    let value = run.result.as_ref().unwrap().py_repr();
    assert!(value.ends_with("3, True, 'err\\n')"), "{value}");
    assert_eq!(run.spawned.len(), 2);
}

// ---------------------------------------------------------------------------
// Findings that shape a design.
// ---------------------------------------------------------------------------

/// A read-write mount of the target exposes `.git`, which template files can
/// never reach. `.git/config` can name programs Git runs later.
#[test]
fn target_mount_exposes_git_dir() {
    let f = fixture();
    let code = r#"
from pathlib import Path
c = Path('.git/config')
c.write_text(c.read_text() + '\tfsmonitor = touch PWNED\n')
Path('.git/hooks/pre-commit').write_text('#!/bin/sh\ntouch PWNED\n')
'written'
"#;
    let run = run_hook(code, &f.target, HookOptions::default());
    show("git-open", &run);
    assert!(run.result.is_ok());
    assert!(fs::read_to_string(f.target.join(".git/config")).unwrap().contains("fsmonitor"));
    // Monty has no chmod, so the planted Git hook is not executable.
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(f.target.join(".git/hooks/pre-commit")).unwrap().permissions().mode();
    println!("[git-open] pre-commit mode {mode:o}");
    assert_eq!(mode & 0o111, 0);
}

/// A `.git` path-component filter in the host closes the direct route.
#[test]
fn git_filter_blocks_direct_paths() {
    let f = fixture();
    let options = HookOptions { deny_git: true, ..HookOptions::default() };
    for code in [
        "open('.git/config', 'a').write('x')",
        "open('/target/.git/config').read()",
        "open('src/../.git/config').read()",
        "import os\nos.rename('README.md', '.git/hooks/pre-commit')",
        "from pathlib import Path\nlist(Path('.git').iterdir())",
    ] {
        let run = run_hook(code, &f.target, options);
        show("git-filter", &run);
        assert_eq!(run.error_type().as_deref(), Some("PermissionError"), "{code}");
    }
    assert_eq!(fs::read_to_string(f.target.join(".git/config")).unwrap(), "[core]\n\tbare = false\n");
}

/// A name filter cannot see through a relative symlink that already exists in
/// the project, because the mount follows in-mount relative symlinks.
#[test]
fn git_filter_bypassed_by_existing_symlink() {
    let f = fixture();
    std::os::unix::fs::symlink(".git", f.target.join("vcs")).unwrap();
    let options = HookOptions { deny_git: true, ..HookOptions::default() };
    let run = run_hook("open('vcs/config', 'a').write('\\tfsmonitor = x\\n')", &f.target, options);
    show("git-filter-symlink", &run);
    assert!(run.result.is_ok());
    assert!(fs::read_to_string(f.target.join(".git/config")).unwrap().contains("fsmonitor"));
}

/// Monty cannot create a symlink, so a hook cannot build that route itself.
#[test]
fn hook_cannot_create_symlink() {
    let f = fixture();
    for code in [
        "import os\nos.symlink('.git', 'vcs')",
        "from pathlib import Path\nPath('vcs').symlink_to('.git')",
        "from pathlib import Path\nPath('README.md').chmod(0o755)",
    ] {
        let run = run_hook(code, &f.target, HookOptions::default());
        show("symlink-create", &run);
        assert!(run.result.is_err(), "{code}");
    }
    assert!(fs::symlink_metadata(f.target.join("vcs")).is_err());
}

#[test]
fn infinite_loop_stops_at_duration_limit() {
    let f = fixture();
    let options = HookOptions { max_duration: Some(std::time::Duration::from_millis(200)), ..HookOptions::default() };
    let run = run_hook("while True:\n    pass", &f.target, options);
    show("loop", &run);
    assert_eq!(run.error_type().as_deref(), Some("TimeoutError"));
    assert!(run.elapsed < std::time::Duration::from_secs(2));
}

/// In-process, `max_memory` is enforced only when the host process installs
/// `monty_alloc::LimitedAllocator` as its global allocator. Without it the
/// limit is inert and only the duration limit stops the hook.
#[test]
fn memory_limit_inert_in_process() {
    let f = fixture();
    let options = HookOptions {
        max_memory: Some(16 * 1024 * 1024),
        max_duration: Some(std::time::Duration::from_millis(500)),
        ..HookOptions::default()
    };
    let run = run_hook("x = []\nwhile True:\n    x.append('a' * (100000 + len(x)))", &f.target, options);
    show("alloc", &run);
    assert_eq!(run.error_type().as_deref(), Some("TimeoutError"));
}

#[test]
fn large_write_is_unbounded_without_write_limit() {
    let f = fixture();
    let options = HookOptions { max_memory: Some(16 * 1024 * 1024), ..HookOptions::default() };
    let run = run_hook("for i in range(200):\n    open(f'big{i}.txt', 'w').write('a' * 1000000)", &f.target, options);
    show("disk", &run);
    let total: u64 = fs::read_dir(&f.target)
        .unwrap()
        .filter_map(|e| e.ok()?.metadata().ok())
        .map(|m| m.len())
        .sum();
    println!("[disk] bytes on disk {total}");
    assert!(run.result.is_ok());
    assert!(total >= 200_000_000);
}
