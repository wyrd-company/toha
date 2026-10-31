---
title: Using Toha from Rust
order: 15
product: toha
relationships:
  describes: toha
---

Toha exposes the interview engine and application operations as a Rust library.
You can resolve templates, manage installed templates, run resumable interviews,
plan and apply output, and update generated projects without enabling the CLI.

## Add the dependency

Add the library without the CLI dependencies:

```sh
cargo add toha --no-default-features
```

To use a reviewed integration commit before it is released, pin it as a Git
dependency instead:

```toml
[dependencies.toha]
git = "https://github.com/wyrd-company/toha"
rev = "<full 40-hex commit>"
default-features = false
```

Replace the placeholder with the full 40-hex commit you reviewed; a branch name
does not freeze the API. This route does not require a registry release.

## Drive an interview

From this repository checkout, drive the same interview with an identity-bearing
answers document. The document names the template it answers, which Toha compares
by exact string equality with the formal name the caller has established before
it evaluates any answer:

```rust
use std::path::Path;
use toha::{Interview, Seed, Template};
use toha::context::{
    EnvironmentSnapshot, ExecutionFacts, HostFacts, InvocationContext,
    SelectedTemplate,
};
use toha::protocol::{self, DocumentStep};
use toha::staging::canonical_target;
let template_root = Path::new("docs/examples/demo").canonicalize().unwrap();
let template = Template::load(&template_root).unwrap();
let expected = template_root.to_str().unwrap();
let target = canonical_target(Path::new("./notes")).unwrap();
let seed = Seed {
    now: "2026-01-01T00:00:00Z[UTC]".parse().unwrap(),
    defaults: Default::default(),
    context: InvocationContext::new(
        target,
        SelectedTemplate::new(
            expected.into(), template.name.clone(), Vec::new(), None,
        ),
        HostFacts::capture(),
        ExecutionFacts::new(false, false),
        EnvironmentSnapshot::Unavailable,
    ).unwrap(),
};
let Interview::Asking(pending) = Interview::start(&template, seed).unwrap()
else { panic!("expected questions") };
// The document must name the formal template it answers.
let text = serde_json::json!({
    "template": expected,
    "answers": {"title": "Sample note", "topic": "Research"},
}).to_string();
let DocumentStep::Accepted { interview, .. } =
    protocol::answer_document_once(expected, pending, &text).unwrap()
else { panic!("the document was rejected") };
let Interview::Complete(done) = interview
else { panic!("expected complete interview") };
assert_eq!(done.answers.len(), 2);
```

In-memory callers that build answers directly still use `Pending::answer` with
`RawAnswers` and the raw headless walk; the envelope is only for external JSON.

## Application operations

The library also owns template resolution, installed-template transactions,
owned interview replay and project updates. These APIs accept explicit paths
and return data. They do not parse arguments, prompt or print command reports.

- `resolve::Paths` and `resolve::{load_context, resolve_template,
  resume_template}` load configuration, fetch/cache templates and restore pins.
- `catalog::Catalog` loads one registry snapshot. Load a fresh snapshot for each
  mutation. The functions `catalog::{add, list, update, remove, add_alias,
  remove_alias, trust, untrust}` return structured events and errors.
- `operations::Session` owns a loaded template and a serializable `StagedRecord`.
  `state` returns a question document, completion or an ended interview; `answer`
  returns per-question rejections without changing rejected state. `seed_warnings`
  returns dropped application-default warnings as data for the caller to report.
  `plan` and `apply` use the existing planner and caller-supplied `HookRunner`.
- `updates::prepare` opens and verifies an update or baseline.
  `updates::Preparation` splits source/base admission from clock/environment
  acquisition when the caller needs deferred inputs. `UpdateSession`
  admits current document inputs, pins the application and replays old answers.
  Its question document omits answers already covered by valid recorded values.
  `resume` checks that the pinned application still matches; `apply` accepts
  the caller's runner and preserves staged state on preview or failure.
  `UpdateSession::apply` and `Prepared::finish` refuse prior edit-owned region drift by
  default. Their `apply_with_options` and `finish_with_options` variants accept
  `updates::UpdateOptions { force, dry_run }` for an explicit choice on one
  request. Force replaces a drifted region; dry-run previews that choice without
  project writes. The refusal includes removed and whole-file-covered prior
  regions. An edit-to-whole transition reports a conflict for differing outside
  bytes even with region force. Persisted trust and answers do not retain force authorization.
- `bundled` exposes the offline demo; `assets` lists, reads and exports the
  embedded agent guidance without enabling the CLI feature.

## Save and resume sessions

Persist or return the session's `record`, then drop the loaded request. Rebuild
with `Session::resume` or `UpdateSession::resume` for each subsequent request.
The record includes the exact template/base, context, time, document values and
accepted submissions. A bare submissions list does not carry all replay inputs.
Reconstruction does not read new document paths. Inspect `state` before acquiring
new submissions or call `admit_documents` to admit new inputs. `operations::Preparation`
checks requirements and documents before its environment-context admission;
`Session::start` accepts an already captured context. Its `admission_answers`
argument admits document values and parts; `answer`/`answer_all` submit answers.
`persist` owns accepted progress and abort cleanup. Document answers are read
fresh when starting an update and stay fixed within that update session.

`UpdateSession::start` admits and submits its `overrides` alongside the recorded
answers. In contrast, `Session::start` uses `admission_answers` only to admit
documents and parts; call `answer` or `answer_all` to submit interview values.

## Use worker threads

Owned request inputs, records and `SessionState` results can cross a worker
boundary. A loaded update retains a thread-local Git repository and is not
`Send` with the current Git dependency features. Construct, consume and drop
that operation inside `spawn_blocking`; return the owned response/record. No
unsafe `Send` implementation or globally shared repository is needed.

## Supply credentials for HTTPS remotes

A credential resolver answers the HTTPS Git remotes of one operation that ask
for authentication, so a caller can hand Toha short-lived tokens without a
credential helper process or a file on disk. Run the operation inside
`with_credential_resolver`:

```rust
use std::sync::Arc;
use toha::source::{Credential, CredentialRequest, with_credential_resolver};

let resolver = Arc::new(|request: &CredentialRequest| {
    if request.host != "git.example.invalid" || request.port != 443 {
        return Err("no credentials for this origin".to_owned());
    }
    if request.path != "/example/templates.git" {
        return Err("no credentials for this repository".to_owned());
    }
    let token = issue_token().map_err(|_| "no token for this repository".to_owned())?;
    Ok(Some(Credential::new("x-access-token", token)))
});
let resolved = with_credential_resolver(resolver, || {
    toha::resolve::resolve_template(&address, &config, &registry, &paths, &cwd)
});
```

The resolver applies to every HTTPS connection the operation opens on the
calling thread: template fetches, commit lookups and the `git` question's
remote probe. It stops when the operation returns or unwinds. Concurrent
operations on other threads each use their own resolver, or none. SSH and
local remotes never consult it.

Toha asks only when an HTTPS remote challenges, once for each challenge. The
`CredentialRequest` names the origin that challenged (`scheme`, `host` and
`port`) and the repository `path` on it. When a redirect reaches another
origin before the challenge, the request names that origin. A credential is
never sent to another origin.

- `Ok(Some(credential))` answers that challenge.
- `Ok(None)` leaves the challenge to the credentials of the Git
  configuration, as without a resolver.
- `Err(reason)` fails the operation with `reason`. Do not put a secret in it.
  A refused `git` question remote stops the apply before it writes, with
  `GitTargetError::CredentialRefused`, and `git_target::probe` reports
  `RemoteState::Refused`.

The password stays out of every Toha error and report, and the `Debug` output
of `Credential` hides it. So does the Basic encoding of `username:password`
that an HTTPS request carries, in standard and URL-safe base64, with or without
padding, when a remote sends it back. Toha removes these as plain text, as Rust
escapes them in a quoted string, and as the decimal byte list that a Git
transport error shows for what a remote sent. The same applies to a credential
from the Git configuration that answers a challenge while a resolver is in
scope. A remote that sends a credential back in another form, such as another
encoding, is outside this protection. Toha does not store or erase the
credential. While a
resolver is in scope, curl's verbose output (`GIT_CURL_VERBOSE` or
`gitoxide.http.verbose`) is off for the operation's HTTP connections, as it
writes the request headers that carry the credential. A clone inside the scope
does not fetch a remote branch named `HEAD`, so an explicit
`@refs/heads/HEAD` selection fails inside the scope. Outside a resolver scope,
that selection works.

## Choose a hook runner and account for process inputs

`HookRunner::run` receives `PlannedHook` and the absolute target. The hook has
argv/program, target-relative working directory, template root and capture
metadata; the runner returns status and optional raw captured streams. There is
no per-hook environment map. Choose the runner and trust decision explicitly.
Operation-owned reports are silent; runners retain their own I/O contract. The
existing Git SSH transport starts the configured/default `ssh` executable and
can forward SSH diagnostics to stderr. Explicit Toha paths are not process
isolation: gix can read global Git configuration through HOME/XDG, executable
requirements and the process runner use PATH, and applied-record operations
retain the TOHA_RECORD process override.

## Run the complete example

Run the complete direct-library recipe from a repository checkout:

```sh
task test:consumer
```

[`tests/consumer/src/main.rs`](https://github.com/wyrd-company/toha/blob/main/tests/consumer/src/main.rs) is a separate Cargo
consumer using `default-features = false`. It creates and resumes an interview,
records a generation with a custom hook runner, rebuilds an update inside a
worker thread, admits a fresh document, previews and applies it, preserves
operator edits, removes a part and resolves an older pinned template. It also
exercises template management and embedded assets.
