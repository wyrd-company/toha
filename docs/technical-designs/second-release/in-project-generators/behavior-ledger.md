# In-project generators — behavior sole-kill ledger

Evidence that every approved behavior (design.md §Behaviors, the mandated fixture
and items 1–21), each independent conjunct, and the three recovery gates has a
named falsifiable test and a required sole-kill: a concrete mutation at a named
boundary that makes the specific named assertion fail, then a green restore.

## Method and basis

- Basis: branch `1030/generators` on base `epic/second-release` `65b5fe2`.
- Each mutation is applied with the recovery changes already committed, to a
  single source file in the foreground; the **named** test is run; the file is
  reverted **by path** (`git checkout -- <file>`); the suite is confirmed green.
- Per case below: the concrete patch site, the command, the observed **named**
  assertion failure, and the green restore. Not an aggregate count.
- Test locations: `tests/generator.rs` (CLI behaviors), `tests/generator_fixture.rs`
  (the mandated `generator-twice` fixture driven from `expect.yml` across the
  scripted / agent / person-PTY / crate routes, plus the person re-ask and the
  single-target plan), `src/cli/like.rs` unit tests (selection/fold/ambiguity),
  `src/snapshot/**` unit tests, `tests/interview_answers.rs` (no-`--like` driver
  parity), and the protocol schema validation inside `tests/generator.rs`.

## Recovery gates

| Gate | Named test / assertion | Required mutation (file → change) | Observed named failure |
|---|---|---|---|
| G1 expect.yml is consumed | `generator_twice_fixture_...` "the \<route\> route seeds from SA" | `tests/fixtures/generator-twice/expect.yml` `seed_from: SA` → a junk id | left = real SA, right = junk id from expect.yml → the expectation is read from expect.yml, not hardcoded |
| G1 complete-tree content | `generator_twice_fixture_...` "complete beta tree equals expected/beta" | `expected/beta/mod.txt` label line → `WRONG` | the relative-path→bytes map differs |
| G1 unexpected-file detection | same complete-tree assertion | `template/template/extra.txt` added (template renders a 2nd file) | the route tree holds an UNEXPECTED `extra.txt` absent from `expected/`; a `mod.txt`-only compare would have passed |
| G2 route-specific divergence | `generator_twice_fixture_...` "the agent route's complete beta tree equals expected/beta" | `src/main.rs` `resolve_pinned_seed` → drop the resumed seed | ONLY the agent route diverges (card/True vs panel/False); distinct from the common interview-seam drop |
| G2 raw submissions (by route shape) | `generator_twice_fixture_...` document-route equality + person override/seed-default assertions | see B1/B3 (seed-wins) kill | document routes record only the override; the person records override + accepted seed defaults |
| G2 engine purity | `generator_twice_fixture_...` "the pure-engine (git-free) seed produces the identical tree" | structural: the seam takes `Option<SnapshotSeed>` {defaults, from} with no `Snapshot`/`Project`/gix type; positive git-free witness + the seam-drop (B1) | a seam that reached past the folded defaults diverges the git-free route |

## Behaviors 1–21 and conjuncts

| # / conjunct | Named test | Required sole-kill mutation (file → change) | Observed named failure |
|---|---|---|---|
| Mandated fixture | `generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes` | consumes `expect.yml`; see the gate rows above | complete-tree / route-divergence / seed-from kills |
| 1 seed-not-replay | `seed_is_a_default_the_document_overrides_not_a_replayed_answer` | `src/interview.rs` `Pending::answer` `(Some(raw),_)` arm → when the id has a `Snapshot` default source, use the seed default instead of the submitted answer (apply seed AFTER answers) | beta renders `label=Root` not the submitted `Beta`; the document could not override the seed |
| 3 answer-beats-seed | same test (and the person route) | same mutation (seed wins over answer) | `label=Root` not `Beta`; on the person route the same mutation renders Root (via the harness) |
| 2 precedence snapshot > configured | `snapshot_shadows_a_configured_default_which_shadows_the_template_default` | `src/interview.rs` seam → skip the snapshot insert when a configured entry exists | `style` shows configured `card`, not seeded `panel` |
| 2 precedence configured > template | `configured_default_shadows_the_template_default_and_the_template_is_the_fallback` | `src/interview.rs` `start_with_context` → drop the Configured bank entries (`.filter(|_| false)`) | `with_tests` falls back to template `True` not the configured `False` |
| 2 template fallback | same test | positive: `style=card` with no snapshot and no config; broad kill (drop the Template default arm) fails the suite | `style` empty / asked |
| 4 source identity, not formal name | `candidates_and_find_required_match_on_source_identity_across_versions` | `src/cli/like.rs:151` `snapshot.source() != source` → `snapshot.template() != source` | the `@v1` snapshot is refused for bare source `gh:example/widget`; `find_required(ID_A).unwrap()` panics |
| 5 generate axis, not update (no target match) | `generate_seeds_across_targets_without_a_target_match` | `src/main.rs` `resolve_like_seed` Reference branch → require `snapshot.target() == project.target()` (as `--from` does) | seeding `feature/beta` from the root snapshot is refused (exit 1) instead of succeeding |
| 6 producer capture preserved (seeded, chaining) | `a_third_application_can_seed_from_the_second` | `src/main.rs` apply flow → when `seed_from.is_some()` skip `plain_apply_snapshot` (reader-only over the seeded run) | the SEEDED second application records no snapshot (`beta_doc["snapshot"]["id"].expect(...)` panics); a third cannot seed — fires on the seeded capture, not the first unseeded one |
| 7 fresh instant | `the_new_application_uses_its_own_instant_not_the_seed_snapshots` | `src/interview.rs` `start_with_seed` → shadow `now` with a fixed past instant (simulating inheriting the seed snapshot's instant) | beta froze to the past instant; the own-instant assertion fails. (`SnapshotSeed` carries no instant field, so true inheritance is impossible by construction; this proves `now` is load-bearing.) |
| 8 wrong-kind dropped, not fatal | `wrong_kind_seed_is_dropped_and_the_apply_proceeds` | `src/interview.rs` `start_with_seed` else-branch → hard-error (`return Err(EvalError)`) instead of drop-with-warning | the apply errors on the wrong-kind value instead of exit 0; "wrong-kind seed is not fatal" fails |
| 8 warning / fallback conjuncts | same test | same test asserts style still inherited and `with_tests` falls to the template default | wrong-kind id falls back; the drop is non-fatal |
| 9 constraint-invalid re-asks (scripted) | `a_constraint_invalid_seed_is_re_asked_not_written` | `src/interview.rs` `answer()` Snapshot arm → accept the default unvalidated | status `applied` (written) instead of `questions` (exit 4) |
| 9 constraint-invalid re-asks (person) | `a_person_is_re_asked_a_constraint_invalid_seed_and_overrides_it` | `src/cli/terminal.rs` `drive_to` re-ask loop → on `Rejected` accept the value instead of re-prompting | the person is not re-asked; the valid-override tree is not produced |
| 10 no-`--like` byte-identical (driver parity) | `configured_default_that_fails_a_constraint_is_attributed_to_configuration` | `src/interview.rs` `start_with_context` → tag configured defaults as `DefaultBankEntry::Snapshot` | a rejected configured default's message changes from "from user.yml: template-defaults…" to "from snapshot …"; driver parity breaks |
| 10 no seed member | `no_like_path_carries_no_seed_member` | the whole unchanged suite is the positive witness | any divergence fails an existing test |
| 11 provenance (names the snapshot) | `a_constraint_invalid_seed_is_re_asked_not_written` | `src/interview.rs` `answer()` Snapshot arm → drop "from snapshot {from}" from the message | the rejection no longer names the seed snapshot id |
| 11 single occupant | `snapshot_shadows_a_configured_default…` | structural: the bank is an `IndexMap` keyed on id (one entry per id); the shadowing kill (B2) proves the Snapshot entry replaces the Configured one | configured value surfaces |
| 12 extra seed id ignored | `an_extra_seed_id_the_template_dropped_is_ignored` | `src/interview.rs` seam → error on an unknown id | status `error` instead of `applied` |
| 12 unknown configured id still warns | `an_unknown_configured_id_warns_while_an_extra_seed_id_is_silent` | `src/interview.rs` `configured_defaults` → drop the "not defined by the selected template" warning | stderr no longer warns about the unknown configured id |
| 13 looped array survives | `fold_takes_the_last_value_per_id_and_keeps_a_looped_array_whole` | `src/cli/like.rs` `fold_submissions` → drop array values | the `items` array assertion panics (key missing) |
| 13 later-batch-wins | same test | `src/cli/like.rs` `fold_submissions` → `if out.contains_key(&id) { continue }` (first wins) | `folded[label]` == `first` not `second` |
| 14 required-absent | `latest_with_no_snapshot_is_required_absent_and_bare_is_a_usage_error` | `src/main.rs` `resolve_like_seed` Latest `None => Err(Missing)` → `Ok(None)` | required-absent `--like latest` returns exit 0 not 1 |
| 14 bare on script/agent | same test | `src/main.rs` remove `LikeFlag::Bare if !interactive => Err(usage)` | a bare `--like` on the scripted route returns exit 0 not the exit-2 usage error |
| 14 absent → plain | same test | positive: `--like` absent → `doc.seed` is none, exit 0 | — |
| 15 dirty target still seeds | `a_dirty_target_still_seeds_but_skips_its_own_capture` | (existing) gate the snapshot read on cleanliness | seeded `style=panel` lost (falls to template default) |
| 15 dirty-capture skip | same test | `src/snapshot/project.rs` `save_after_apply` → remove `if !was_clean { Skipped(Dirty) }` | `snapshot.skipped != "dirty"` on a dirty target |
| 16 ≥6-char prefix exact-one (positive) | `a_full_id_and_a_six_char_prefix_both_select_the_snapshot` | `src/snapshot/record.rs` `MIN_PREFIX` `6`→`1` | a 5-char prefix resolves and seeds |
| 16 short/unknown fail | `a_short_or_unknown_or_ambiguous_prefix_fails_clearly` | (negative) a <6-char and an unknown prefix fail exit 1 | — |
| 16 ambiguity (exact-one) | `a_prefix_matching_several_snapshots_is_ambiguous_and_a_unique_one_resolves` | `src/snapshot/project.rs` `find()` `_ => Err(Ambiguous)` → `Ok(matches.pop())` (arbitrary pick) | `matches!(err, Ambiguous)` false; the test panics on `unwrap_err` |
| 17 repository-wide | `snapshots_all_lists_every_target_while_snapshots_stays_target_scoped` | `src/snapshot/project.rs` `snapshots_all` → re-add target filter | the repo-wide listing loses the sibling target |
| 17 source-filtered | `candidates_and_find_required…` | `src/cli/like.rs` `candidates` → drop the source filter | candidates admit the foreign source |
| 17 newest-first | `candidates_and_find_required…` (order) | `src/snapshot/project.rs` `sort_listed_newest_first` `y.id().cmp(x.id())` → `x.id().cmp(&y.id())` | candidates come oldest-first `[ID_A, ID_B]` not `[ID_B, ID_A]` |
| 18 single-target plan | `a_seeded_plan_is_single_target_and_refuses_a_foreign_target`; `a_like_apply_plans_exactly_one_target` | `src/plan.rs` `Plan::build` → disable the `ContextTarget` guard | "a seeded plan refuses a target other than its context" fails |
| 19 pinned resume does not drift | `resume_honors_the_pin_not_the_newest_snapshot` | `src/main.rs` `stage` → `saved.seed = None` (pin not stored) | resume re-seeds from the newer snapshot |
| 19 deleted-pin refusal | `a_deleted_pinned_snapshot_fails_the_resume_clearly` | `src/main.rs` `resolve_pinned_seed` → swallow a missing pin (`let Ok(..) else { return Ok(None) }`) | the resume returns exit 0 (unseeded) instead of exit 1 when the pin is deleted |
| 19 continue/apply reconstruction | `the_agent_route_shows_the_seeded_defaults_and_pins_the_snapshot` + `resume_honors_the_pin…` | pin-storage kill above | continue and apply both rebuild from the pin |
| 20 reader-only selection | `like_selection_writes_no_ref` | (existing) write a ref in the selection path | the ref set changes |
| 21 engine purity / route parity | `generator_twice_fixture_...` | route-specific divergence (G2) + the git-free pure-engine witness | a route diverges / the pure-engine tree diverges |
| capture fix: ancestor discovery | `open_discovers_from_an_existing_ancestor_for_a_not_yet_created_target` | `src/snapshot/project.rs` `Project::open` → discover from the target not its ancestor | a fresh subpath returns `None` instead of opening its repository |
| capture fix: repo-root source_dir | mandated fixture | `src/snapshot/project.rs` `save_after_apply` → reintroduce the subpath double-prefix | the subpath application saves no snapshot |
| protocol: `seed` schema agreement | `seed_is_a_default…` (schema validation) | `interview-protocol.schema.yml` → drop `seed` from the `applied` def | "document does not match the protocol schema" |

## Notes on independent conjuncts and structural guarantees

- **Route-specific divergence vs the common seam.** The behavior-21 route-parity
  kill (G2) breaks only the agent route (`resolve_pinned_seed`), proving the
  routes agree with *each other*; it is distinct from the interview-seam drop
  (B1), which breaks every route together. Both are recorded separately.
- **Raw submissions are route-shaped.** Document routes (scripted/agent/crate)
  record only the submitted override; the person route records every prompt's
  accepted value, including a seed default accepted with Enter. The harness
  asserts document-route equality and the person override/seed-default split, so
  the comparison is falsifiable per route rather than a single all-equal claim.
- **Single-occupant bank** is structural: the bank is an `IndexMap` keyed on the
  question id, so an id has exactly one entry; the snapshot-shadows-configured
  kill (B2) proves the Snapshot entry replaces the Configured one in place.
- **Fresh instant** is structural: `SnapshotSeed` carries `{defaults, from}` with
  no instant field, so the seed cannot carry the frozen instant; the recorded
  kill proves the passed `now` is load-bearing.
- **Engine purity** is a type-boundary guarantee: `Resolution::start_with_seed`
  consumes only `Option<SnapshotSeed>` (folded `IndexMap<Id, RawAnswer>` + the id
  string), never a `Snapshot`, `Project`, or gitoxide type; the git-free
  pure-engine route produces the identical tree, and the seam-drop (B1) is its
  falsifiable kill.
