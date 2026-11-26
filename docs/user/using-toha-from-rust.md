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

A resolver or a credential helper is the only way to give Toha a password. Toha
refuses a git URL that holds one, as in `https://user:password@host/repo.git`:
`source::parse` and `source::fetch` return `SourceError::Address`, a `git`
answer is rejected, and `git_target::preflight` and `git_target::initialize`
return `GitTargetError::RemotePassword`, before anything connects or writes.
`source::has_password` tells whether a URL holds one.

## Choose the SSH key for SSH remotes

Without a choice, Toha's SSH remotes run the configured `ssh` program, which
offers every key of the agent at `SSH_AUTH_SOCK`. The Git host signs the
client in with the first key it recognises, even one that can't reach the
repository. To make one operation offer only one key of the agent, run it
inside `with_ssh_key`:

```rust
use toha::source::{SshKey, with_ssh_key};

// The public key in authorized-keys form, as `ssh-add -L` prints it.
let key = SshKey::parse("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA...")?;
let resolved = with_ssh_key(key, || {
    toha::resolve::resolve_template(&address, &config, &registry, &paths, &cwd)
});
```

The key applies to every SSH connection the operation opens on the calling
thread: template fetches, commit lookups and the `git` question's remote
probe. It stops when the operation returns or unwinds. Concurrent operations
on other threads each use their own key, or none. HTTPS and local remotes
never use it. It combines with `with_credential_resolver` and
`with_stop_signal` in any order.

`SshKey::parse` takes the key type, the base64 key and an optional comment.
The key is picked from the agent at `SSH_AUTH_SOCK`, read when the operation
connects; `SshKey::with_agent(socket)` names another agent socket instead.

Before each SSH connection, Toha asks the agent for its keys. When the agent
doesn't hold the key, or can't be reached, the connection fails without
trying any other key. A fetch returns `SourceError::Git` and the `git`
question's probe reports `RemoteState::Unreachable`, with a message that names
the key by its type and fingerprint, as `ssh-keygen -l` prints it:

```text
git fetch failed: the SSH agent at /run/agent.sock does not offer the key ssh-ed25519 SHA256:nQpreSffQ9djmqcCYLK2VAzke74gO1kFga1ppEfjsaI
```

Otherwise the connection runs the configured SSH program (`core.sshCommand`,
`GIT_SSH_COMMAND` or `GIT_SSH`, else `ssh` on `PATH`) with the OpenSSH options
`IdentitiesOnly=yes`, `IdentityFile` naming a temporary file that holds the
public key, and `IdentityAgent` naming the agent socket. Toha decides whether
a remote is SSH after Git's `url.<base>.insteadOf` rewrites, so an HTTPS URL
rewritten to SSH uses the key too.

The rest of the SSH configuration still applies, and OpenSSH would also offer
an identity it names. So before it connects, Toha runs the same command with
`ssh -G`, which prints the configuration for the remote's host. When that
configuration names another `IdentityFile`, as `ssh -i` or an SSH config file
can, or sets `IdentitiesOnly` or `IdentityAgent` to something else, the
connection fails before it signs in, with a message that names the setting
and the key:

```text
git fetch failed: the SSH configuration for git.example.com sets `identityfile ~/.ssh/id_work`, so the connection would not offer only the key ssh-ed25519 SHA256:nQpreSffQ9djmqcCYLK2VAzke74gO1kFga1ppEfjsaI
```

The program must therefore accept OpenSSH options, including `-G`;
`ssh.variant` is treated as `ssh`. Scoped keys need a Unix agent socket; on
Windows the connection fails.

## Stop a fetch or an apply

A stop signal lets a caller end an operation it no longer needs, such as one
whose request has gone away. Run the operation inside `with_stop_signal` and
call `stop` on the signal, or on any clone of it, from another thread:

```rust
use toha::stop::{StopSignal, with_stop_signal};

let signal = StopSignal::new();
let stopper = signal.clone();
// When the request ends, from any thread:
// stopper.stop();
let resolved = with_stop_signal(signal, || {
    toha::resolve::resolve_template(&address, &config, &registry, &paths, &cwd)
});
```

The signal applies to the operation on the calling thread until it returns or
unwinds: template fetches and commit lookups, document schema fetches while a
template loads, the `git` question's remote probe, an apply and an update. Operations on other threads, and code after the
call, run without it. A signal stays stopped once fired, so give each operation
a new one. Without a signal, every operation behaves as before.

Once the signal fires, the operation returns the `Stopped` variant of its
error, inside the errors that wrap it as any other failure is:

- A fetch returns `source::SourceError::Stopped`, `resolve::ResolveError::Stopped`
  or `catalog::CatalogError::Stopped`. `Session::resume` carries the
  `ResolveError` in `OperationError::Resolve`.
- A template load that the signal ended fails with `LoadError::stopped` set,
  such as while it fetched a document schema. `Session::resume` carries the
  `LoadError` in `OperationError::Load`. `catalog::add` and `catalog::trust`
  return `catalog::CatalogError::Stopped`. `catalog::list` and the trust check
  at the end of `catalog::update` don't fail when a template can't be loaded:
  they report a stopped load as they report any load failure, as not trusted
  (`list`) or `ApprovalLapsed` (`update`), without returning the stop.
- Preparing an update returns `updates::UpdateError::Stopped` when the signal
  fires while its template or merge base is fetched or loaded, or its merge
  base rendered. `UpdateSession::resume` carries it in `updates::SessionError::Update`.
- A probe reports `git_target::RemoteState::Stopped`.
- An apply returns `ApplyError::Stopped`; `Session::apply` carries it in
  `OperationError::Apply`.
- An update returns `applied::MergeError::Stopped`; `UpdateSession::apply`
  carries it in `updates::SessionError::Merge`.

A stopped apply writes no further directory, file, injected edit, repository,
hook run or applied record. The files it wrote before the signal stay, as after
any failed apply. A stopped update writes nothing to the working tree, or
restores the files it already wrote, and leaves the index and applied record
unchanged. Toha checks the signal before each write, so a hook that is already
running finishes first.

A fetch returns as soon as the signal fires, even while the remote holds the
connection without sending anything. Inside the signal's scope, Toha runs the
fetch on a thread of its own, under the operation's credential resolver, into
a temporary folder that thread owns. The calling thread returns at once and the
fetch's destination stays absent. The `git` question's remote probe runs the
same way.

### Limit: the abandoned transfer runs until the remote acts

Toha's Git library, gix, cannot interrupt a transfer that waits for a remote.
The transfer a stop abandons therefore keeps running, without blocking the
caller, until the remote acts:

- Over HTTP(S) it keeps its Toha thread, gix's curl thread and the connection
  until the remote answers or closes the connection, or Git's
  `http.lowSpeedLimit` and `http.lowSpeedTime` end the transfer.
- Over SSH and `file://` it keeps its Toha thread and the `ssh` or
  `git-upload-pack` process until that process answers or exits.

The transfer then ends: it writes nothing more and removes its temporary
folder. Releasing the transfer at the stop is tracked in
[wyrd-company/toha#78](https://github.com/wyrd-company/toha/issues/78).

A document schema fetch has no such limit, apart from its name lookup. Toha
makes the fetch on the calling thread, with its own HTTPS connection, and
checks the signal at least every 100 ms. The stop aborts the fetch and closes
that connection within a fraction of a second, even while the schema endpoint
sends nothing.

### Limit: a pending schema host lookup

A name lookup can't be interrupted. A stop can arrive while a lookup for the
fetch is still pending: the schema host's, or the target's behind a
`socks4://` or `socks5://` proxy, which curl looks up itself. The fetch then
returns within 2 s, but the threads that wait on that lookup run on:

- curl's resolver threads, which are detached rather than joined;
- behind such a proxy, one Toha thread named `toha-schema-release`
  (`toha-schema-rel` on Linux, which keeps 15 bytes of a thread name), which
  finishes releasing curl's handles. Toha waits for it at most 250 ms.

These threads hold no connection, because the stop has already closed it.
They end by themselves when the system resolver returns. A stop with no
pending lookup leaves no thread. With a libcurl that supports neither of
curl's quick-exit options, the stop waits for the lookup instead. Making the
lookup itself stoppable is tracked in
[wyrd-company/toha#81](https://github.com/wyrd-company/toha/issues/81).

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
