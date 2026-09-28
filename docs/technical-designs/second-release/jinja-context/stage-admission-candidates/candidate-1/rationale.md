---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Rationale

## Problem

Stage must decide whether a template can observe five gated values before interview progress, yet ordinary source-tree Jinja is currently compiled later and MiniJinja undeclared-variable results miss `debug()`, self-shadowing reads, and includes. Later batches cannot ask for trust or reread ambient facts. The design must retain the producer-owned `CanonicalTarget` factory, all seventeen context values, and the existing pure interview engine.

## Usage (caller's view)

The template author writes `{{ toha_env_editor }}` or `{{ debug() }}` in any supported render source. `toha stage --trust sample ./output` admits and captures needed values once; `toha continue ./output answers.json` and staged `toha apply ./output` replay that snapshot. A new direct apply uses `--trust` or an eligible matching live review. The command caller loads `Template`, calls `template.admit_environment(decision, reader)`, constructs `InvocationContext` and `Seed`, then calls `Interview::start`. A direct crate caller performs the same calls with typed facts and a supplied reader. The detailed call sites in `design.md` are the interface contract.

## Shape

`Template` owns an immutable compiled-source catalog containing every configuration render, source-tree path/body, and literal include closure. Its load-time AST walk computes a five-bit `EnvNeeds` invariant of that catalog. Admission has one method: decision plus five-value reader in, typed snapshot or attributed refusal out. Planning renders those exact compiled objects. This hides discovery, alias analysis, source identity, and failure attribution behind the first seam a caller already crosses, per codebase-design depth and boundary-discipline. The caller still owns the trust decision and host capture because those facts differ by entry point. `EnvironmentSnapshot` stores only referenced fixed slots; staging maps it to private versioned wire data and reconstructs the target through the sole factory. The engine carries immutable context and projects one Jinja map, per single-source-of-truth and encode-lessons-in-structure.

## Synthesis decision

This capsule requests one candidate, so no arena synthesis or other runner evidence exists. The retained compiled-source catalog from `direction.md` is the base because it makes admitted analysis and later rendering refer to the same bytes. The referenced-only wire snapshot follows the needs invariant. A broader context/source serialization was considered and rejected below.

## Tradeoffs accepted

- We accept eager source-tree traversal and earlier attributed load errors in exchange for admission before interview progress.
- We accept conservative `debug()` and alias analysis, including reads in unreachable branches, in exchange for sound admission without removing supported Jinja syntax.
- We accept plaintext referenced values in existing staged JSON until apply, abort, or operator removal in exchange for deterministic later batches without ambient reads.
- We accept that a matching `HookSurface` review does not attest non-hook Jinja content in exchange for preserving the approved direct access policy and its present review scope.
- We accept a hard legacy restage error in exchange for no invented context facts and no replay-time access path.

## Alternatives considered

- **All five fixed values after any reference:** simple wire decoding, but it reads and retains unneeded plaintext. It hides little additional complexity from the caller because the same admission seam remains.
- **Admitted source/context snapshot in the staged record:** freezes more bytes across processes, but duplicates the selected template revision and enlarges the wire contract. The retained catalog plus existing selected-revision replay contract keeps the public interface smaller; unrecoverable source identity fails before replay.
- **Analyze, then reopen sources during planning:** initially smaller implementation, but makes callers coordinate a content-identity check or a later access decision. That leaks the security invariant across modules and cannot support deterministic replay after mutation.
- **Exact-name scan on legacy records:** smaller apparent compatibility path, but `debug()` observes context without exact-name references. It cannot establish that missing historical facts are harmless.

## Open questions and risks

- Can the existing selected-template revision mechanism recover the exact staged source revision for every supported template source, or must a missing revision fail replay as specified?
- Does the MiniJinja AST expose enough callable alias information for the conservative walk, and which supported syntax requires a more conservative all-five classification?
- Is the existing staged-state directory and umask acceptable for the explicit plaintext lifetime described here?

## Next implementation step

Build the `Template` compiled-source catalog and its AST needs tests before wiring command admission or staged serialization.
