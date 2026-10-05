//! Which Python operations a file-editing hook needs does Monty support?
//! Prints a matrix; asserts only the operations the spike claims work.

use std::fs;

use monty_hooks_spike::{HookOptions, run_hook};

const OPS: &[(&str, &str)] = &[
    ("re-sub-groups", "import re\nre.sub(r'(\\w+)=(\\d+)', r'\\2=\\1', 'a=1 b=2')"),
    ("re-sub-callable", "import re\nre.sub(r'\\d+', lambda m: str(int(m.group()) + 1), 'v1.2.3')"),
    ("re-multiline", "import re\nre.findall(r'^## (.+)$', '## a\\nx\\n## b', re.M)"),
    ("json-roundtrip", "import json\njson.dumps(json.loads('{\"b\":1,\"a\":[1,2]}'), indent=2, sort_keys=True)"),
    ("fstring-format", "n = 3\nf'{n:03d} {\"x\":>4} {1.5:.2f}'"),
    ("str-methods", "'  Abc-Def  '.strip().lower().replace('-', '_').split('_')"),
    ("splitlines-join", "'\\n'.join(l for l in 'a\\nb\\n'.splitlines() if l)"),
    ("dict-comprehension", "{k: v for k, v in zip('ab', range(2))}"),
    ("sorted-key", "sorted(['b', 'A', 'c'], key=str.lower)"),
    ("dataclass", "from dataclasses import dataclass\n@dataclass\nclass P:\n    x: int\nP(1).x"),
    ("class-def", "class A:\n    def f(self):\n        return 1\nA().f()"),
    ("exceptions", "try:\n    open('missing.txt').read()\nexcept FileNotFoundError:\n    'missing'"),
    ("path-exists", "from pathlib import Path\nPath('README.md').exists()"),
    ("path-iterdir", "from pathlib import Path\nsorted(p.name for p in Path('.').iterdir())"),
    ("path-glob", "from pathlib import Path\nsorted(str(p) for p in Path('.').glob('*.md'))"),
    ("path-rglob", "from pathlib import Path\nsorted(str(p) for p in Path('.').rglob('*.txt'))"),
    ("os-walk", "import os\n[d for d, _, _ in os.walk('.')]"),
    ("path-suffix-stem", "from pathlib import Path\n(Path('a/b.tar.gz').suffix, Path('a/b.txt').stem, Path('a/b').parent.name)"),
    ("path-rename-in", "from pathlib import Path\nPath('src/main.txt').rename('src/renamed.txt')\nPath('src/renamed.txt').exists()"),
    ("path-unlink-in", "from pathlib import Path\nPath('src/x.txt').write_text('x')\nPath('src/x.txt').unlink()\nPath('src/x.txt').exists()"),
    ("rmdir-in", "from pathlib import Path\nPath('d').mkdir()\nPath('d').rmdir()"),
    ("shutil", "import shutil"),
    ("tomllib", "import tomllib"),
    ("string-template", "import string"),
    ("textwrap", "import textwrap"),
    ("csv", "import csv"),
    ("hashlib", "import hashlib"),
    ("bytes-io", "from pathlib import Path\nPath('b.bin').write_bytes(b'\\x00\\x01')\nPath('b.bin').read_bytes()"),
    ("open-readlines", "open('README.md').readlines()"),
    ("open-iterate", "[l for l in open('README.md')]"),
    ("with-open", "with open('README.md') as fh:\n    t = fh.read()\nlen(t)"),
    ("datetime-now", "from datetime import date\ndate.today().year > 2000"),
    ("print", "print('hello from a hook')"),
    ("sys-argv", "import sys\nsys.argv"),
    ("input-answers", "answers['name']"),
];

#[test]
fn subset_matrix() {
    let mut rows = Vec::new();
    for (label, code) in OPS {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path();
        fs::create_dir_all(target.join("src")).unwrap();
        fs::write(target.join("README.md"), "# T\nline\n").unwrap();
        fs::write(target.join("src/main.txt"), "m\n").unwrap();
        let run = run_hook(code, target, HookOptions { answers: &[("name", "widget")], ..HookOptions::default() });
        let outcome = match &run.result {
            Ok(v) => format!("OK   {}", v.py_repr()),
            Err(e) => format!("FAIL {}", e.to_string().lines().last().unwrap_or_default()),
        };
        println!("[subset] {label:<18} {outcome}");
        rows.push((*label, run.result.is_ok()));
    }
    let ok = |l: &str| rows.iter().any(|(r, ok)| *r == l && *ok);
    for required in ["re-sub-groups", "input-answers", "json-roundtrip", "path-iterdir", "path-rename-in", "with-open"] {
        assert!(ok(required), "{required}");
    }
}
