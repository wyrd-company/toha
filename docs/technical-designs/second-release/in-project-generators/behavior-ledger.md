# In-project generators — behavior sole-kill ledger

Evidence that every approved behavior (design.md §Behaviors, the mandated fixture
and items 1–21), each independent conjunct, and the recovery gates has a named
falsifiable test and a required sole-kill: a concrete mutation at a named boundary
that makes the specific named assertion fail (not a compilation or unrelated-test
failure), then a green restore.

## Method and basis

- Basis: branch `1030/generators` on base `epic/second-release` `65b5fe2`.
- Each mutation is applied with the recovery changes already committed, to a single
  source file in the foreground; the **named** test is run; the file is reverted
  **by path** (`git checkout -- <file>`); the suite is confirmed green.
- Per-case commands, the observed named-assertion failure (with its
  `tests/…:line`), the restore command, and the committed execution HEAD are
  retained in `task1030-evidence/soul-kills.md` beside the CI logs.
- Test locations: `tests/generator.rs` (CLI behaviors), `tests/generator_fixture.rs`
  (the mandated `generator-twice` fixture driven from `expect.yml` across the
  scripted / agent / person-PTY / crate routes, the person re-ask, and the
  single-target plan), `src/cli/like.rs` and `src/snapshot/**` unit tests, and
  `tests/interview_answers.rs` (no-`--like` driver parity).

## Recovery gates

| Gate | Named test / assertion | Required mutation (file → change) | Observed named failure |
|---|---|---|---|
| G1 expect.yml is consumed | `generator_twice_fixture_…` "the \<route\> route seeds from SA" | `expect.yml` `seed_from: SA` → junk id | left = real SA, right = junk id from expect.yml |
| G1 complete-tree content | "complete beta tree equals expected/beta" | `expected/beta/mod.txt` → WRONG | the relative-path→bytes map differs |
| G1 unexpected-file | same complete-tree assertion | add `template/template/extra.txt` | the route tree holds an UNEXPECTED file absent from `expected/` |
| G2 route-specific divergence | "the agent route's complete beta tree equals expected/beta" | `main.rs` `resolve_pinned_seed` → drop the resumed seed | ONLY the agent route diverges; distinct from the common seam |
| G1 actual pre-format raw | `routes_record_the_pre_format_raw_submission_not_the_formatted_answer` | `main.rs` `plain_apply_snapshot` → persist the formatted `completed.answers` instead of the raw `submissions` | under a non-identity `format` (upper), the actual persisted snapshot records "BETA" not the raw "beta"; the RAW assertion fails while the tree is unchanged (generator_fixture.rs) |
| G2/B21 engine purity | "the pure-engine (git-free, bogus-from) seed produces the identical tree" / "== the git-backed crate route" | `interview.rs` `start_with_seed` → `if seed.from.len() != 26 { fall back }` (resolve/gate on `from` identity) | the bogus-`from` git-free route drops its defaults while the git-backed crate route still seeds → they diverge (boundary identity-leak detection, not a drop-default-everywhere) |

## Behaviors 1–21 and conjuncts

| # / conjunct | Named test | Required sole-kill mutation (file → change) | Observed named failure |
|---|---|---|---|
| Mandated fixture | `generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes` | the gate rows above | — |
| 1 seed-not-replay / 3 answer-beats-seed | `seed_is_a_default_the_document_overrides_not_a_replayed_answer` | `interview.rs` `Pending::answer` `(Some(raw),_)` → use the Snapshot default instead of the submitted answer (seed after answers) | beta renders `label=Root` not the submitted `Beta` |
| 2 leg 1 snapshot > configured | `snapshot_shadows_a_configured_default_which_shadows_the_template_default` | `interview.rs` seam → `if defaults.contains_key(&id) { continue }` (configured shadows snapshot) | style shows configured, not seeded panel (generator.rs:673) |
| 2 leg 2 configured > template | `configured_default_shadows_the_template_default_and_the_template_is_the_fallback` | `interview.rs` `start_with_context` → drop the Configured bank entries | with_tests falls to template True not configured False |
| 2 leg 3 template fallback | same test | `interview.rs` `render_default` Text/Select arm → `.filter(|_| false)` (drop the template default) | style != "card" (generator.rs:1120) |
| 4 source identity, not formal name | `candidates_and_find_required_match_on_source_identity_across_versions` | `like.rs:151` `snapshot.source() != source` → `snapshot.template() != source` | the `@v1` snapshot is refused for the bare source |
| 5 generate axis, not update (no target match) | `generate_seeds_across_targets_without_a_target_match` | `main.rs` resolve_like_seed Reference → require `snapshot.target() == project.target()` | seeding feature/beta from the root snapshot is refused (exit 1) |
| 6 producer capture preserved (seeded, chaining) | `a_third_application_can_seed_from_the_second` | `main.rs` apply flow → when `seed_from.is_some()` skip `plain_apply_snapshot` | the SEEDED second application records no snapshot; a third cannot seed |
| 7 fresh instant | `the_new_application_uses_its_own_instant_not_the_seed_snapshots` | carry `snapshot.generated().get().clone()` through `cli::like::LikeSeed.generated` and use it as `now` on the scripted apply path (`main.rs`) — the ACTUAL frozen generated instant, not the ULID creation time | beta renders when=2020-01-01 (the inherited `snapshot.generated`), not its own 2022 (generator.rs:816) |
| 8 wrong-kind dropped, not fatal | `wrong_kind_seed_is_dropped_and_the_apply_proceeds` | `interview.rs` seam else → hard-error instead of drop | the apply errors instead of exit 0 |
| 8 wrong-kind WARNING conjunct | same test | `interview.rs` seam → remove the `warnings.push` (silent drop) | the warning assertion fails (generator.rs:402) |
| 8 wrong-kind FALLBACK conjunct | same test | `interview.rs` seam → `if true || parse_kind(...)` (accept any kind) | the fallback assertion (tests="unknown") fails (generator.rs:398) |
| 9 constraint-invalid re-asks (scripted) | `a_constraint_invalid_seed_is_re_asked_not_written` | `interview.rs` `answer()` Snapshot arm → accept the default unvalidated | status `applied` instead of `questions` (exit 4) |
| 9 constraint-invalid re-asks (PERSON) | `a_person_is_re_asked_a_constraint_invalid_seed_and_overrides_it` | `cli/terminal.rs` `drive_to` re-ask loop → accept the rejected value instead of re-prompting | the person is not re-asked; the valid-override tree is not produced |
| 10 no-`--like` byte-identical (driver parity) | `configured_default_that_fails_a_constraint_is_attributed_to_configuration` | `interview.rs` `start_with_context` → tag configured defaults as `Snapshot` | a rejected configured default's message changes to "from snapshot …" |
| 10 no seed member | `no_like_path_carries_no_seed_member` | `main.rs` `with_seed` → always insert a `seed` member | a plain apply carries a seed member (generator.rs:179) |
| 11 provenance (names the snapshot) | `a_constraint_invalid_seed_is_re_asked_not_written` | `interview.rs` `answer()` Snapshot arm → drop "from snapshot {from}" | the rejection no longer names the seed snapshot id |
| 11 single occupant / duplicate preparation | `a_snapshot_seeded_id_is_prepared_exactly_once_with_a_single_occupant` (tests/interview_answers.rs) | `interview.rs` prompt-push site → push the seeded prompt TWICE into `batch.items` (an ordered Vec) | the seeded id is prepared twice (style_prompts == 2); exactly-once occupancy fails (interview_answers.rs) |
| 12 extra seed id ignored | `an_extra_seed_id_the_template_dropped_is_ignored` | `interview.rs` seam → error on an unknown id | status `error` instead of `applied` |
| 12 extra seed id SILENCE | same test | `interview.rs` seam unknown-id branch → `warnings.push(...)` | stderr warns about the extra snapshot id (generator.rs:938) |
| 12 unknown CONFIGURED id warns | `an_unknown_configured_id_warns_while_an_extra_seed_id_is_silent` | `interview.rs` `configured_defaults` → drop the "not defined" warning | stderr no longer warns |
| 13 looped array survives | `fold_takes_the_last_value_per_id_and_keeps_a_looped_array_whole` | `like.rs` `fold_submissions` → drop array values | the `items` array assertion panics |
| 13 later-batch-wins | same test | `like.rs` `fold_submissions` → `if out.contains_key(&id) { continue }` | `folded[label]` == first not second |
| 14 required-absent | `latest_with_no_snapshot_is_required_absent_and_bare_is_a_usage_error` | `main.rs` resolve_like_seed Latest `None => Err(Missing)` → `Ok(None)` | required-absent `--like latest` returns exit 0 not 1 |
| 14 bare on script route | same test | `main.rs` remove the `Bare if !interactive` guard | a bare `--like` on the scripted route returns exit 0 not 2 |
| 14 bare on AGENT route | `a_bare_like_on_the_agent_route_is_a_usage_error` | `main.rs` resolve_like_seed `Bare => Some(Latest)` (silently seed) | the agent bare case returns exit 4, not the exit-2 usage error (generator.rs:1174) |
| 14 ordinary absent selector | `latest_with_no_snapshot_is_required_absent_and_bare_is_a_usage_error` | `main.rs` `LikeFlag::Absent => Ok(None)` → `Some(Latest)` | a plain apply with --like absent errors/!=0 (generator.rs:454) |
| 15 dirty target still seeds | `a_dirty_target_still_seeds_but_skips_its_own_capture` | gate the snapshot read on cleanliness | seeded style lost (template default) |
| 15 dirty-capture skip | same test | `project.rs` save_after_apply → remove the `if !was_clean { Skipped(Dirty) }` | `snapshot.skipped != "dirty"` |
| 16 full-id / unique resolves | `a_full_id_and_a_six_char_prefix_both_select_the_snapshot` | `project.rs` find() `1 => Ok` → `1 => Err(Ambiguous)` | the full id / unique prefix no longer resolves (generator.rs:251) |
| 16 short prefix fails | `a_short_or_unknown_or_ambiguous_prefix_fails_clearly` | `record.rs` `MIN_PREFIX` 6 → 1 | a 5-char prefix resolves and seeds (generator.rs:277) |
| 16 unknown reference refusal | same test | `main.rs` resolve_like_seed Reference → swallow find error (Ok(None)) | an unknown reference proceeds unseeded instead of exit 1 (generator.rs:277) |
| 16 ambiguity exact-one | `a_prefix_matching_several_snapshots_is_ambiguous_and_a_unique_one_resolves` | `project.rs` find() `_ => Err(Ambiguous)` → `Ok(matches.pop())` | a shared prefix resolves arbitrarily instead of Ambiguous |
| 17 repository-wide | `snapshots_all_lists_every_target_while_snapshots_stays_target_scoped` | `project.rs` snapshots_all → re-scope to self.rel | the repo-wide listing loses the sibling target |
| 17 source-filtered | `candidates_and_find_required_…` | `like.rs` candidates → drop the source filter | candidates admit the foreign source |
| 17 newest-first | `candidates_and_find_required_…` (order) | `project.rs` sort_listed_newest_first → ascending | candidates come oldest-first |
| 18 single-target plan | `a_seeded_plan_is_single_target_and_refuses_a_foreign_target`; `a_like_apply_plans_exactly_one_target` | `plan.rs` Plan::build → disable the `ContextTarget` guard | "a seeded plan refuses a target other than its context" (generator_fixture.rs:1052) |
| 19 pinned resume does not drift | `resume_honors_the_pin_not_the_newest_snapshot` | `main.rs` stage → `saved.seed = None` | resume re-seeds from the newer snapshot |
| 19 deleted-pin refusal (apply) | `a_deleted_pinned_snapshot_fails_the_resume_clearly` | `main.rs` resolve_pinned_seed → swallow a missing pin | the apply resume proceeds unseeded instead of exit 1 |
| 19 continue reconstruction (independent) | `a_deleted_pin_fails_continue_independently_of_apply` | `main.rs` resolve_pinned_seed → swallow a missing pin | continue proceeds instead of exit 1 (generator.rs:1219) |
| 20 reader-only selection | `like_selection_writes_no_ref` | `main.rs` resolve_like_seed Latest → `project.remove(&[*snapshot.id()], true)` | the ref set changes (generator.rs:1046) |
| 21 engine purity / route parity | `generator_twice_fixture_…` | the G2/B21 purity + route-divergence rows above | — |
| capture fix: ancestor discovery | `open_discovers_from_an_existing_ancestor_for_a_not_yet_created_target` | `project.rs` Project::open → discover from the target not its ancestor | a fresh subpath returns None |
| capture fix: repo-root source_dir | `generator_applied_twice_into_two_subpaths_second_seeds_first` | `project.rs` save_after_apply → capture from `workdir.join(self.rel)` | the subpath application saves no snapshot (generator.rs:1336) |
| protocol: `seed` schema agreement | `seed_is_a_default_…` (assert_valid) | `interview-protocol.schema.yml` → drop `seed` from the applied def | "document does not match the protocol schema" (generator.rs:141) |

## Notes on representation and independent conjuncts

- **Common effective-raw parity (pre-format).** Each route exposes `effective`
  from the actual completed interview: the crate route reads `Completed::answers`
  in-process (no manufactured `vec![submission]`); the CLI routes re-drive the same
  engine seam with their own recorded submissions. The harness asserts all routes'
  effective values equal the seed+override, keeps the route-shaped explicit
  submission logs as a separate assertion, and the complete-tree equality catches a
  lost override before the effective block.
- **Route-shaped explicit logs.** Document routes record only the submitted
  override; the person route records every accepted value (override + accepted seed
  defaults). This is a legitimate per-route distinction, asserted separately from
  the common effective parity.
- **Engine purity is identity-unaware, not merely type-structural.** The git-free
  route uses a `from` that is not a real id; the kill makes the seam resolve/gate on
  `from`, so the git-free route diverges from the git-backed crate route while the
  latter is unaffected — a boundary identity-leak witness, not a drop-default.
- **Fresh instant** is proven by inheriting the seed snapshot's instant (decoded
  from its ULID id) at the seam, not by substituting an arbitrary fixed instant.
- **Single occupant** is proven on the prepared-bank surface (the staged questions
  default is the snapshot value), killed by making the seam keep the configured
  occupant — distinct from the rendered-file precedence check.

## Round 5 strengthening (per the four material gaps)

- **Actual pre-format raw observation.** The replay-based effective comparison is
  removed. `routes_record_the_pre_format_raw_submission_not_the_formatted_answer`
  observes each real route's ACTUAL persisted snapshot (the design's raw pre-format
  submission record) under a non-identity `format`, so the raw submission ("beta")
  differs from the rendered answer ("BETA"); its kill (`plain_apply_snapshot`
  recording formatted answers) fails the RAW assertion specifically, not the tree.
- **B7 inherits `snapshot.generated`.** The kill carries the actual frozen
  generated instant (2020) through `LikeSeed.generated`, not the ULID creation time.
- **B11 occupancy** is proven at the `batch.items` Vec boundary (prepared exactly
  once), where a duplicate IS representable — not in a keyed JSON property.
- **Per-case targets.** Every case records its correct cargo target: `--test
  generator`, `--test generator_fixture`, `--bin toha cli::like::tests::…`, `--lib
  snapshot::project::tests::…`, or `--test interview_answers`. Full commands,
  observed named failures, committed HEAD, and restore results are retained in
  `task1030-evidence/soul-kills.md`.
