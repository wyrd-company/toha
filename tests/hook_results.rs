//! Opt-in Jinja hook results (task 1063): the named behaviours, load errors, and
//! JSON mode of `docs/technical-designs/hook-results.yml`.
//!
//! Templates are built in a tempdir and driven end to end — load, interview,
//! plan, apply — with a scripted runner that returns configured exit codes and
//! captured bytes, so the failure gate, strict-UTF-8 decode, JSON parse, and
//! result projection are exercised on the real path.

use std::{cell::RefCell, collections::HashMap, path::Path};

use toha::{
    Applied, ApplyError, ApplyOptions, Interview, Plan, Planned, Seed, Template,
    apply::OutputFault,
    context::InvocationContext,
    hook::{HookError, HookOutcome, HookRunner},
    plan::PlannedHook,
    staging,
};

/// Writes `template.yml` and an (empty) `template/` source dir into a tempdir,
/// plus any extra files. The tempdir must outlive load and apply.
fn tree(yaml: &str, extra: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), yaml).unwrap();
    for (rel, body) in extra {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

/// Loads a template, returning the aggregated load-error text.
fn load_err(yaml: &str) -> String {
    load_err_with(yaml, &[])
}
fn load_err_with(yaml: &str, extra: &[(&str, &str)]) -> String {
    let dir = tree(yaml, extra);
    match Template::load(dir.path()) {
        Ok(_) => panic!("expected a load error"),
        Err(error) => error.to_string(),
    }
}
/// Loads a template, asserting it loads cleanly.
fn loads(yaml: &str) {
    let dir = tree(yaml, &[]);
    Template::load(dir.path()).unwrap_or_else(|e| panic!("expected clean load: {e}"));
}

/// The configured outcome for one producer id.
#[derive(Clone)]
struct Out {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
/// A runner that returns configured outcomes by hook id and records every argv.
#[derive(Default)]
struct Scripted {
    outcomes: HashMap<String, Out>,
    calls: RefCell<Vec<Vec<String>>>,
}
impl Scripted {
    fn new() -> Self {
        Self::default()
    }
    fn set(mut self, id: &str, out: Out) -> Self {
        self.outcomes.insert(id.into(), out);
        self
    }
    /// A clean exit whose stdout is `stdout`.
    fn ok(self, id: &str, stdout: &str) -> Self {
        self.set(
            id,
            Out {
                success: true,
                code: Some(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: vec![],
            },
        )
    }
    /// A nonzero exit `code` whose stdout is `stdout`.
    fn exit(self, id: &str, code: i32, stdout: &str) -> Self {
        self.set(
            id,
            Out {
                success: false,
                code: Some(code),
                stdout: stdout.as_bytes().to_vec(),
                stderr: vec![],
            },
        )
    }
    /// A termination by signal (no exit code).
    fn signal(self, id: &str) -> Self {
        self.set(
            id,
            Out {
                success: false,
                code: None,
                stdout: vec![],
                stderr: vec![],
            },
        )
    }
    /// A clean exit whose stdout is the raw bytes `stdout`.
    fn ok_bytes(self, id: &str, stdout: &[u8]) -> Self {
        self.set(
            id,
            Out {
                success: true,
                code: Some(0),
                stdout: stdout.to_vec(),
                stderr: vec![],
            },
        )
    }
    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }
    /// The recorded argv whose program (argv[0]) equals `program`.
    fn call(&self, program: &str) -> Option<Vec<String>> {
        self.calls().into_iter().find(|argv| argv[0] == program)
    }
}
impl HookRunner for Scripted {
    fn run(&self, hook: &PlannedHook, _target: &Path) -> Result<HookOutcome, HookError> {
        self.calls.borrow_mut().push(hook.argv());
        let out = hook
            .id
            .as_ref()
            .and_then(|id| self.outcomes.get(id.as_str()))
            .cloned()
            .unwrap_or(Out {
                success: true,
                code: Some(0),
                stdout: vec![],
                stderr: vec![],
            });
        // A stream is captured (piped to Toha) only when the hook declares it, as
        // the real ProcessRunner does.
        Ok(HookOutcome {
            success: out.success,
            code: out.code,
            stdout: hook.capture.stdout.then_some(out.stdout),
            stderr: hook.capture.stderr.then_some(out.stderr),
        })
    }
}

const TRUSTED: ApplyOptions = ApplyOptions {
    force: true,
    trusted: true,
};

/// Loads and completes the interview for a hook-only template (no questions), so
/// the interview completes at start.
fn build(dir: &Path) -> (Template, staging::CanonicalTarget, tempfile::TempDir) {
    let template = Template::load(dir).expect("load");
    let target_dir = tempfile::tempdir().unwrap();
    let target = staging::canonical_target(target_dir.path()).unwrap();
    (template, target, target_dir)
}
fn plan_of(template: &Template, target: &staging::CanonicalTarget) -> Plan {
    let Interview::Complete(completed) = Interview::start(
        template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
            context: InvocationContext::for_target(target.clone()),
        },
    )
    .expect("interview") else {
        panic!("expected a complete interview")
    };
    Plan::build(template, &completed, target).expect("plan build")
}
/// Applies `yaml` with `runner` and returns the outcome, keeping temp dirs alive.
#[allow(clippy::result_large_err)]
fn apply(yaml: &str, runner: &Scripted) -> Result<Applied, ApplyError> {
    let dir = tree(yaml, &[]);
    let (template, target, _target_dir) = build(dir.path());
    let plan = plan_of(&template, &target);
    plan.apply(&target, TRUSTED, runner)
}
/// The after-apply text of a successful apply.
fn after_apply(yaml: &str, runner: &Scripted) -> Option<String> {
    match apply(yaml, runner).expect("apply") {
        Applied::Written { after_apply, .. } => after_apply,
        Applied::NeedsTrust(_) => panic!("unexpected NeedsTrust"),
    }
}

// ---------------------------------------------------------------------------
// A real conditional flow (design §1.1, §1.2).
// ---------------------------------------------------------------------------

const CONDITIONAL: &str = "\
name: sample
hooks:
  - id: lint
    run: [ lint-tool, --check ]
    allow-failure: true
  - run: [ fix-tool ]
    when: \"lint.exit_code != 0\"
  - run: [ test-tool ]
    when: \"lint.exit_code == 0\"
";

#[test]
fn a_later_hook_runs_when_an_earlier_exit_code_is_nonzero() {
    let runner = Scripted::new().exit("lint", 1, "");
    apply(CONDITIONAL, &runner).expect("apply");
    assert!(
        runner.call("fix-tool").is_some(),
        "fix runs on nonzero lint"
    );
    assert!(
        runner.call("test-tool").is_none(),
        "test skipped on nonzero"
    );
}

#[test]
fn a_later_hook_skips_when_an_earlier_exit_code_is_zero() {
    let runner = Scripted::new().ok("lint", "");
    apply(CONDITIONAL, &runner).expect("apply");
    assert!(
        runner.call("fix-tool").is_none(),
        "fix skipped on zero lint"
    );
    assert!(runner.call("test-tool").is_some(), "test runs on zero lint");
}

#[test]
fn captured_stdout_reaches_a_later_argument_and_after_apply() {
    let yaml = "\
name: sample
hooks:
  - id: version
    run: [ describe ]
    capture: [ stdout ]
  - run: [ note, \"{{ version.stdout }}\" ]
messages:
  after-apply: \"built {{ version.stdout }}\"
";
    let runner = Scripted::new().ok("version", "v1.4.0\n");
    let after = after_apply(yaml, &runner);
    // Trailing newline stripped, as a shell `$(…)` does.
    assert_eq!(after.as_deref(), Some("built v1.4.0"));
    assert_eq!(runner.call("note").unwrap()[1], "v1.4.0");
}

// ---------------------------------------------------------------------------
// Default failure behaviour (design §5 invariant 5).
// ---------------------------------------------------------------------------

#[test]
fn a_nonzero_hook_without_allow_failure_stops_and_fails() {
    let yaml = "\
name: sample
hooks:
  - id: build
    run: [ build-tool ]
  - run: [ after ]
";
    let runner = Scripted::new().exit("build", 2, "");
    let error = apply(yaml, &runner).unwrap_err();
    assert!(matches!(error, ApplyError::Hook { .. }), "{error:?}");
    assert!(runner.call("after").is_none(), "later hook must not run");
}

#[test]
fn allow_failure_tolerates_a_nonzero_exit_code() {
    let yaml = "\
name: sample
hooks:
  - id: build
    run: [ build-tool ]
    allow-failure: true
  - run: [ after ]
    when: \"build.exit_code == 2\"
";
    let runner = Scripted::new().exit("build", 2, "");
    apply(yaml, &runner).expect("tolerated");
    assert!(
        runner.call("after").is_some(),
        "apply continues, code readable"
    );
}

#[test]
fn a_signal_is_fatal_even_with_allow_failure() {
    let yaml = "\
name: sample
hooks:
  - id: build
    run: [ build-tool ]
    allow-failure: true
  - run: [ after ]
    when: \"build.exit_code is not none\"
";
    let runner = Scripted::new().signal("build");
    let error = apply(yaml, &runner).unwrap_err();
    assert!(matches!(error, ApplyError::Hook { .. }), "{error:?}");
    assert!(runner.call("after").is_none(), "signal stops the apply");
}

// ---------------------------------------------------------------------------
// Not-run producers read `none` (design §5 invariant 3, §5A.4).
// ---------------------------------------------------------------------------

#[test]
fn a_skipped_text_producer_reads_none_in_every_field() {
    let yaml = "\
name: sample
data:
  do_it: false
hooks:
  - id: probe
    run: [ probe-tool ]
    capture: [ stdout ]
    when: do_it
  - run: [ note, \"{{ probe.stdout }}\", \"{{ probe.exit_code }}\" ]
    when: \"probe.exit_code is none\"
";
    let runner = Scripted::new().ok("probe", "unused");
    apply(yaml, &runner).expect("apply");
    assert!(runner.call("probe-tool").is_none(), "probe skipped");
    assert_eq!(runner.call("note").unwrap(), ["note", "", ""]);
}

#[test]
fn a_read_through_a_none_json_id_is_defined() {
    let yaml = "\
name: sample
data:
  do_it: false
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
    status-id: pkg_status
    when: do_it
  - run: [ note, \"{{ pkg.name }}\", \"{{ pkg_status.exit_code }}\" ]
    when: \"pkg_status.exit_code is none\"
";
    let runner = Scripted::new().ok("pkg", "{\"name\":\"x\"}");
    apply(yaml, &runner).expect("apply");
    // pkg is none, so pkg.name reads through none to empty; metadata is none.
    assert_eq!(runner.call("note").unwrap(), ["note", "", ""]);
}

// ---------------------------------------------------------------------------
// JSON mode value types (design §5A.1).
// ---------------------------------------------------------------------------

fn json_after_apply(stdout: &str, read: &str) -> Option<String> {
    let yaml = format!(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
messages:
  after-apply: \"{read}\"
"
    );
    let runner = Scripted::new().ok("pkg", stdout);
    after_apply(&yaml, &runner)
}

#[test]
fn json_binds_each_value_type_at_the_id() {
    assert_eq!(
        json_after_apply("{\"name\":\"demo\"}", "{{ pkg.name }}").as_deref(),
        Some("demo")
    );
    assert_eq!(
        json_after_apply("[10, 20, 30]", "{{ pkg[1] }}").as_deref(),
        Some("20")
    );
    assert_eq!(
        json_after_apply("\"plain\"", "{{ pkg }}").as_deref(),
        Some("plain")
    );
    assert_eq!(json_after_apply("42", "{{ pkg }}").as_deref(), Some("42"));
    // MiniJinja renders booleans as `True`/`False`, as it does for any boolean.
    assert_eq!(
        json_after_apply("true", "{{ pkg }}").as_deref(),
        Some("True")
    );
    // JSON null is a parsed `none`, so the message renders empty and is dropped.
    assert_eq!(
        json_after_apply("null", "value={{ pkg }}").as_deref(),
        Some("value=")
    );
}

#[test]
fn a_parsed_json_string_is_data_not_a_template() {
    // The parsed string contains Jinja syntax; it must not be re-rendered.
    assert_eq!(
        json_after_apply("\"{{ oops }}\"", "{{ pkg }}").as_deref(),
        Some("{{ oops }}")
    );
}

// ---------------------------------------------------------------------------
// JSON parse faults (design §5A.5, §5A.6).
// ---------------------------------------------------------------------------

fn json_apply_error(stdout: &str) -> ApplyError {
    let yaml = "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
messages:
  after-apply: \"{{ pkg }}\"
";
    let runner = Scripted::new().ok("pkg", stdout);
    apply(yaml, &runner).unwrap_err()
}

#[test]
fn exit_zero_empty_stdout_is_a_byte_free_fatal_fault() {
    let error = json_apply_error("");
    assert!(
        matches!(
            error,
            ApplyError::HookOutput {
                fault: OutputFault::Empty,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn exit_zero_malformed_json_is_a_byte_free_fatal_fault() {
    for bad in [
        "{not json",
        "\u{feff}{\"a\":1}",  // leading BOM
        "{\"a\":1}{\"b\":2}", // multiple documents
    ] {
        let error = json_apply_error(bad);
        assert!(
            matches!(
                error,
                ApplyError::HookOutput {
                    fault: OutputFault::NotJson { .. },
                    ..
                }
            ),
            "{bad:?}: {error:?}"
        );
        // No captured bytes appear in the message.
        let text = error.to_string();
        assert!(!text.contains("not json"), "{text}");
        assert!(!text.contains("\"a\""), "{text}");
    }
}

#[test]
fn a_tolerated_nonzero_unparseable_stdout_is_lenient() {
    let yaml = "\
name: sample
hooks:
  - id: findings
    run: [ lint-tool ]
    parse: json
    capture: [ stdout ]
    status-id: lint
    allow-failure: true
  - run: [ note, \"{{ lint.parsed }}\", \"{{ lint.stdout }}\", \"{{ findings }}\" ]
    when: \"lint.exit_code != 0\"
";
    let runner = Scripted::new().exit("findings", 1, "not-json-output");
    apply(yaml, &runner).expect("apply continues");
    // <id> is none, status.parsed is false, and the raw text stays in status.stdout.
    assert_eq!(
        runner.call("note").unwrap(),
        ["note", "False", "not-json-output", ""]
    );
}

#[test]
fn an_untolerated_nonzero_with_bad_json_reports_apply_error_hook() {
    // Failure is checked before parse, so the error is Hook, not HookOutput.
    let yaml = "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
messages:
  after-apply: \"{{ pkg }}\"
";
    let runner = Scripted::new().exit("pkg", 1, "not json");
    let error = apply(yaml, &runner).unwrap_err();
    assert!(matches!(error, ApplyError::Hook { .. }), "{error:?}");
}

#[test]
fn an_untolerated_failure_is_reported_before_any_decode() {
    // Failure is checked strictly before decode: an untolerated nonzero hook with
    // a non-UTF-8 captured stream reports Hook, never HookOutput.
    let yaml = "\
name: sample
hooks:
  - id: probe
    run: [ probe-tool ]
    capture: [ stdout ]
  - run: [ note, \"{{ probe.stdout }}\" ]
    when: \"probe.exit_code == 0\"
";
    let mut runner = Scripted::new();
    runner = runner.set(
        "probe",
        Out {
            success: false,
            code: Some(1),
            stdout: vec![0xff, 0xfe],
            stderr: vec![],
        },
    );
    let error = apply(yaml, &runner).unwrap_err();
    assert!(matches!(error, ApplyError::Hook { .. }), "{error:?}");
}

// ---------------------------------------------------------------------------
// Encoding and leakage (design §5 invariants 4, 7).
// ---------------------------------------------------------------------------

#[test]
fn a_non_utf8_capture_is_a_byte_free_hook_output_fault() {
    let yaml = "\
name: sample
hooks:
  - id: probe
    run: [ probe-tool ]
    capture: [ stdout ]
  - run: [ note, \"{{ probe.stdout }}\" ]
";
    let runner = Scripted::new().ok_bytes("probe", &[b'D', b'A', b'T', b'A', 0xff, 0xfe]);
    let error = apply(yaml, &runner).unwrap_err();
    match &error {
        ApplyError::HookOutput {
            fault: OutputFault::NotUtf8 { valid_up_to },
            ..
        } => {
            assert_eq!(*valid_up_to, 4)
        }
        other => panic!("{other:?}"),
    }
    assert!(!error.to_string().contains("DATA"), "no bytes in Display");
}

#[test]
fn no_debug_or_display_of_a_hook_type_contains_captured_text() {
    // An untolerated nonzero producer with a captured secret stdout fails with
    // ApplyError::Hook before decode; its Debug and Display must omit the bytes.
    const SECRET: &str = "TOP-SECRET-TOKEN";
    let yaml = "\
name: sample
hooks:
  - id: probe
    run: [ probe-tool ]
    capture: [ stdout ]
  - run: [ note, \"{{ probe.stdout }}\" ]
    when: \"probe.exit_code == 0\"
";
    let runner = Scripted::new().exit("probe", 1, SECRET);
    let error = apply(yaml, &runner).unwrap_err();
    let ApplyError::Hook { outcome, .. } = &error else {
        panic!("expected Hook: {error:?}")
    };
    // The outcome's own Debug omits the captured streams entirely, so neither the
    // readable text nor its byte representation can appear.
    let outcome_debug = format!("{outcome:?}");
    assert!(
        !outcome_debug.contains("stdout"),
        "outcome Debug: {outcome_debug}"
    );
    assert!(
        !outcome_debug.contains("stderr"),
        "outcome Debug: {outcome_debug}"
    );
    let bytes = format!("{:?}", SECRET.as_bytes()[0]); // e.g. "84"
    assert!(
        !outcome_debug.contains(&bytes),
        "outcome Debug leaked bytes"
    );
    assert!(!format!("{error}").contains(SECRET), "Display leaked bytes");
    assert!(!format!("{error:?}").contains(SECRET), "Debug leaked bytes");
}

// ---------------------------------------------------------------------------
// A hook without id is byte-identical (design §5 invariant 8).
// ---------------------------------------------------------------------------

#[test]
fn a_hook_without_id_debugs_exactly_as_before() {
    let yaml = "\
name: sample
hooks:
  - run: [ tool, arg ]
";
    let dir = tree(yaml, &[]);
    let (template, target, _t) = build(dir.path());
    let plan = plan_of(&template, &target);
    let Planned::Ready(hook) = &plan.hooks[0] else {
        panic!("a result-free hook is ready")
    };
    let mut hook = hook.clone();
    // Field-like text inside a path is data, not an added debug field.
    hook.template_root = hook.template_root.join("identity-template");
    let debug = format!("{hook:?}");
    let expected = format!(
        "PlannedHook {{ program: Run([\"tool\", \"arg\"]), cwd: None, template_root: {:?} }}",
        hook.template_root,
    );
    assert_eq!(debug, expected);
}

// ---------------------------------------------------------------------------
// Dry-run / trust listing (design §5 dry-run).
// ---------------------------------------------------------------------------

#[test]
fn a_result_reading_hook_is_retained_and_previews_in_source_form() {
    let yaml = "\
name: sample
hooks:
  - id: version
    run: [ describe ]
    capture: [ stdout ]
  - run: [ note, \"{{ version.stdout }}\" ]
";
    let dir = tree(yaml, &[]);
    let (template, target, _t) = build(dir.path());
    let plan = plan_of(&template, &target);
    assert!(
        matches!(plan.hooks[0], Planned::Ready(_)),
        "producer is ready"
    );
    let Planned::AfterHooks(deferred) = &plan.hooks[1] else {
        panic!("the reader is deferred")
    };
    let (argv, _cwd) = deferred.hook_preview().expect("hook preview");
    // run[0] is the rendered program; later args keep their source placeholder.
    assert_eq!(argv[0], "note");
    assert_eq!(argv[1], "{{ version.stdout }}");
}

#[test]
fn needs_trust_counts_a_deferred_hook() {
    let yaml = "\
name: sample
hooks:
  - id: version
    run: [ describe ]
    capture: [ stdout ]
  - run: [ note, \"{{ version.stdout }}\" ]
";
    let dir = tree(yaml, &[]);
    let (template, target, _t) = build(dir.path());
    let plan = plan_of(&template, &target);
    let untrusted = ApplyOptions {
        force: true,
        trusted: false,
    };
    let applied = plan.apply(&target, untrusted, &Scripted::new());
    assert!(
        matches!(applied, Ok(Applied::NeedsTrust(_))),
        "deferred hooks need trust"
    );
}

// ---------------------------------------------------------------------------
// Load errors — surfaces, ordering, capture (design §5 table).
// ---------------------------------------------------------------------------

#[test]
fn a_forward_result_read_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - run: [ early ]
    when: \"late.exit_code == 0\"
  - id: late
    run: [ late-tool ]
",
    );
    assert!(
        error.contains("hook result read before hook `late` runs: late"),
        "{error}"
    );
}

#[test]
fn a_self_result_read_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: loop
    run: [ tool ]
    when: \"loop.exit_code == 0\"
",
    );
    assert!(
        error.contains("hook result read before hook `loop` runs: loop"),
        "{error}"
    );
}

#[test]
fn a_result_read_in_a_file_body_is_a_load_error() {
    let error = load_err_with(
        "\
name: sample
hooks:
  - id: lint
    run: [ lint-tool ]
",
        &[("template/out.txt", "{{ lint.exit_code }}\n")],
    );
    assert!(
        error.contains("readable only in a later top-level hook"),
        "{error}"
    );
    assert!(error.contains("lint"), "{error}");
}

#[test]
fn a_result_read_in_before_apply_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: lint
    run: [ lint-tool ]
messages:
  before-apply: \"{{ lint.exit_code }}\"
",
    );
    assert!(
        error.contains("readable only in a later top-level hook"),
        "{error}"
    );
}

#[test]
fn a_result_read_in_an_interview_field_is_a_load_error() {
    let error = load_err(
        "\
name: sample
interview:
  - id: q
    type: text
    prompt: \"{{ lint.exit_code }}\"
hooks:
  - id: lint
    run: [ lint-tool ]
",
    );
    assert!(
        error.contains("readable only in a later top-level hook"),
        "{error}"
    );
}

#[test]
fn a_result_read_in_run_zero_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pick
    run: [ pick-tool ]
    capture: [ stdout ]
  - run: [ \"{{ pick.stdout }}\", arg ]
",
    );
    assert!(
        error.contains("readable only in a later top-level hook"),
        "{error}"
    );
}

#[test]
fn a_result_read_in_each_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pick
    run: [ pick-tool ]
    capture: [ stdout ]
  - run: [ tool, \"{{ item }}\" ]
    each: \"pick.stdout as item\"
",
    );
    assert!(
        error.contains("readable only in a later top-level hook"),
        "{error}"
    );
}

#[test]
fn a_read_stream_that_is_not_captured_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: version
    run: [ describe ]
  - run: [ note, \"{{ version.stdout }}\" ]
",
    );
    assert!(
        error.contains("hook `version` does not capture stdout; add `capture: [stdout]`"),
        "{error}"
    );
}

#[test]
fn a_captured_stream_that_is_never_read_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: version
    run: [ describe ]
    capture: [ stdout ]
  - run: [ note ]
    when: \"version.exit_code == 0\"
",
    );
    assert!(
        error.contains("captured stdout of hook `version` is never read"),
        "{error}"
    );
}

#[test]
fn allow_failure_without_a_read_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: lint
    run: [ lint-tool ]
    allow-failure: true
",
    );
    assert!(
        error.contains("the result of lint is never read"),
        "{error}"
    );
}

#[test]
fn an_unknown_result_field_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: lint
    run: [ lint-tool ]
  - run: [ note ]
    when: \"lint.nonsense == 0\"
",
    );
    assert!(
        error.contains("unknown hook result field: lint.nonsense"),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// Load errors — authored-field rules (design §5, §5A tables).
// ---------------------------------------------------------------------------

#[test]
fn an_id_with_each_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: many
    run: [ tool ]
    each: \"items as item\"
",
    );
    assert!(
        error.contains("a hook with each cannot declare id"),
        "{error}"
    );
}

#[test]
fn base_field_requirements_are_load_errors() {
    for (yaml_fragment, message) in [
        ("    capture: [ stdout ]", "capture requires id"),
        ("    allow-failure: true", "allow-failure requires id"),
        ("    parse: json", "parse requires id"),
        ("    status-id: meta", "status-id requires id"),
    ] {
        let yaml = format!(
            "\
name: sample
hooks:
  - run: [ tool ]
{yaml_fragment}
"
        );
        let error = load_err(&yaml);
        assert!(error.contains(message), "{yaml_fragment}: {error}");
    }
}

#[test]
fn parse_json_without_capture_stdout_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stderr ]
",
    );
    assert!(
        error.contains("parse: json requires capture: [stdout]"),
        "{error}"
    );
}

#[test]
fn status_id_without_parse_json_is_a_load_error() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    capture: [ stdout ]
    status-id: meta
  - run: [ note ]
    when: \"pkg.exit_code == 0\"
",
    );
    assert!(error.contains("status-id requires parse: json"), "{error}");
}

#[test]
fn a_status_id_equal_to_id_is_a_duplicate() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
    status-id: pkg
",
    );
    assert!(error.contains("duplicate id: pkg"), "{error}");
}

// ---------------------------------------------------------------------------
// JSON attribute-form guard and status-id-required (design §5A.3, §5A.7).
// ---------------------------------------------------------------------------

#[test]
fn the_json_attribute_form_guard_names_the_subscript_escape() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
    status-id: meta
  - run: [ note ]
    when: \"pkg.exit_code == 0\"
",
    );
    assert!(
        error.contains("hook pkg parses stdout as JSON; use pkg['exit_code'] for a JSON key or meta.exit_code for metadata"),
        "{error}"
    );
}

#[test]
fn an_aliased_json_root_degrades_to_a_key_read() {
    // Through an alias the attribute chain no longer roots at `pkg`, so the guard
    // does not fire: `p.exit_code` is a plain key read on the aliased value.
    loads(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
  - run: [ note, \"{% set p = pkg %}{{ p.exit_code }}\" ]
",
    );
}

#[test]
fn a_skippable_json_producer_requires_status_id() {
    // Its own `when` can skip it.
    let own = load_err(
        "\
name: sample
data:
  go: true
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
    when: go
  - run: [ note, \"{{ pkg.name }}\" ]
",
    );
    assert!(own.contains("parse: json hook `pkg` may not run"), "{own}");

    // An enclosing group `when` can skip it (not just its own `when`).
    let group = load_err(
        "\
name: sample
data:
  go: true
interview:
  - group: outer
    when: go
    nodes:
      - hook:
          id: pkg
          run: [ pkg-tool ]
          parse: json
          capture: [ stdout ]
hooks:
  - run: [ note, \"{{ pkg.name }}\" ]
",
    );
    assert!(
        group.contains("parse: json hook `pkg` may not run"),
        "{group}"
    );
}

#[test]
fn allow_failure_on_a_json_hook_requires_a_read_status_id() {
    let error = load_err(
        "\
name: sample
hooks:
  - id: pkg
    run: [ pkg-tool ]
    parse: json
    capture: [ stdout ]
    status-id: meta
    allow-failure: true
  - run: [ note, \"{{ pkg.name }}\" ]
",
    );
    assert!(
        error.contains("the result of meta is never read"),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// An existing template id equal to a hook id still loads (design predecessor).
// ---------------------------------------------------------------------------

#[test]
fn a_hook_id_shares_the_authored_id_space() {
    // A hook `id` collides with a question id as an ordinary duplicate.
    let error = load_err(
        "\
name: sample
interview:
  - id: version
    type: text
    prompt: v
hooks:
  - id: version
    run: [ tool ]
",
    );
    assert!(error.contains("duplicate id: version"), "{error}");
}
