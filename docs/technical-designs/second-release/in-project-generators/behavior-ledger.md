# In-project generators — behavior sole-kill ledger

Evidence that every approved behavior (design.md §Behaviors, the mandated fixture
and items 1–21) and each independent conjunct has a named falsifiable test and a
required sole-kill: a mutation at a named boundary that makes the specific named
assertion fail, then a green restore.

Basis: branch `1030/generators` on base `epic/second-release` `65b5fe2`. Each
mutation was applied with the change already committed, to a single source file
in the foreground, the named test run, then the file reverted by path and the
suite confirmed green. Test locations: `tests/generator.rs` (CLI behaviors),
`tests/generator_fixture.rs` (the `generator-twice` fixture across the person /
scripted / agent / crate routes), `src/cli/like.rs` and `src/snapshot/**` and
`src/snapshot/record` (unit), and the protocol schema validation inside
`tests/generator.rs`.

| # / conjunct | Named test | Sole-kill mutation (file → change) | Observed named failure |
|---|---|---|---|
| Mandated fixture (two subpaths, both snapshots, routes agree) | `generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes` | consumes `tests/fixtures/generator-twice/` assets; seam drop below diverges every route | see row 21 (seam drop) |
| 1 seed-not-replay | `seed_is_a_default_the_document_overrides_not_a_replayed_answer` | `src/interview.rs` seam → drop snapshot defaults (`is_ok()`→`false`) | `seed_is_a_default…` style/with_tests no longer inherited |
| 2 precedence snapshot > configured > template | `snapshot_shadows_a_configured_default_which_shadows_the_template_default` | `src/interview.rs` seam → skip the snapshot insert when a configured entry exists (`if defaults.contains_key(&id) { continue }`) | `snapshot_shadows_a_configured_default…` gets configured `card` not seeded `panel` |
| 3 answer-beats-seed | `seed_is_a_default_the_document_overrides_not_a_replayed_answer` | same seam boundary (route answer applied before default in `answer()`) | override of `label` lost |
| 4 source identity, not formal name | `candidates_and_find_required_match_on_source_identity_across_versions`; `a_foreign_source_snapshot_is_refused_and_not_a_candidate` | `src/cli/like.rs` `find_required` → `snapshot.source() != source`→`false` | foreign-source `WrongSource` no longer raised |
| 5 generate axis, not update (no target match) | `generate_seeds_across_targets_without_a_target_match` | selection keys on `source` only (find_required compares source, never target — see row 4 boundary); a cross-target seed succeeds | positive witness; row-4 mutation shows selection is source-keyed |
| 6 producer capture preserved (chaining) | `a_third_application_can_seed_from_the_second`; fixture (both snapshots) | `src/snapshot/project.rs` `save_after_apply` → reintroduce subpath double-prefix (`workdir.join(self.rel)`) | fixture: "the first subpath application saved a snapshot" fails |
| 7 fresh instant | `the_new_application_uses_its_own_instant_not_the_seed_snapshots` | `src/interview.rs` seam → shadow `now` with a fixed `2000-01-01…` when seeding | "the seeded application uses its own instant" fails |
| 8 wrong-kind dropped, not fatal | `wrong_kind_seed_is_dropped_and_the_apply_proceeds` | `src/interview.rs` seam → accept any kind (`is_ok()`→`true`) | apply errors on the wrong-kind value instead of proceeding |
| 9 constraint-invalid re-asks | `a_constraint_invalid_seed_is_re_asked_not_written` | `src/interview.rs` `answer()` Snapshot arm → accept the default unvalidated (`Ok(v.clone())`) | status `applied` (written) instead of `questions` (exit 4) |
| 10 no-`--like` byte-identical | `no_like_path_carries_no_seed_member` + the entire unchanged pre-existing suite | seam `None` delegates to `start_with_context`; whole suite is the witness | (positive; any divergence fails an existing test) |
| 11 single-occupant bank + provenance | `snapshot_shadows_a_configured_default…` (single occupant); `a_constraint_invalid_seed…` (message names the snapshot) | row-2 boundary (one occupant); `answer()` Snapshot message names `from` | configured value surfaces / message loses the snapshot id |
| 12 extra seed ids ignored | `an_extra_seed_id_the_template_dropped_is_ignored` | `src/interview.rs` seam → error on an unknown id (`else { return Err(…"extra snapshot id") }`) | status `error` kind `render` instead of `applied` |
| 13 looped array preserved | `fold_takes_the_last_value_per_id_and_keeps_a_looped_array_whole` (like.rs) | `src/cli/like.rs` `fold_submissions` → drop array values (`if value.is_array() { continue }`) | the `items` array assertion panics (key missing) |
| 14 required-absent vs bare vs absent | `latest_with_no_snapshot_is_required_absent_and_bare_is_a_usage_error` | `src/main.rs` `resolve_like_seed` Latest → `None => Ok(None)` (silent proceed) | required-absent `latest` gets exit 0 `applied` instead of exit 1 |
| 15 dirty target still seeds | `a_dirty_target_still_seeds_but_skips_its_own_capture` | `src/main.rs` `resolve_like_seed` → gate the read on cleanliness (`if Dirty { return Ok(None) }`) | seeded `style=panel` lost (falls to template default) |
| 16 ≥6-char prefix exact-one | `a_full_id_and_a_six_char_prefix_both_select_the_snapshot` (positive); `a_short_or_unknown_or_ambiguous_prefix_fails_clearly` (negative) | `src/snapshot/record.rs` `MIN_PREFIX` `6`→`1` | a 5-char prefix resolves and seeds (exit 0) instead of failing |
| 17 repository-wide, source-filtered | `snapshots_all_lists_every_target_while_snapshots_stays_target_scoped` (lib); `candidates_and_find_required…` | `src/snapshot/project.rs` `snapshots_all` → re-add target filter; `src/cli/like.rs` `candidates` → drop source filter | repo-wide listing loses the sibling target; candidates admit the foreign source |
| 18 single-target plan | `a_seeded_plan_is_single_target_and_refuses_a_foreign_target`; `a_like_apply_plans_exactly_one_target` | `src/plan.rs` `Plan::build` → disable the `ContextTarget` guard (`if false`) | "a seeded plan refuses a target other than its context" fails |
| 19 pinned resume does not drift | `resume_honors_the_pin_not_the_newest_snapshot` | `src/main.rs` `stage` → `saved.seed = None` (pin not stored) | resume re-seeds from the newer snapshot / loses the pin |
| 20 reader-only selection | `like_selection_writes_no_ref` | `src/main.rs` `resolve_like_seed` → write a ref in selection (`project.remove(id)`) | "the --like selection … write no snapshot ref" fails (ref set changed) |
| 21 engine purity / route parity | `generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes` | `src/interview.rs` seam → drop snapshot defaults (`is_ok()`→`false`) | "scripted route matches expected/beta" fails (all routes diverge from `expected/beta`) |
| capture fix: ancestor discovery | `open_discovers_from_an_existing_ancestor_for_a_not_yet_created_target`; `open_refuses_a_target_outside_the_working_tree` | `src/snapshot/project.rs` `Project::open` → discover from the target, not its ancestor | a fresh subpath returns `None` instead of opening its repository |
| protocol: `seed` schema agreement | `seed_is_a_default…` (schema validation of the `applied` doc) | `interview-protocol.schema.yml` → drop `seed` from the `applied` def | "document does not match the protocol schema" |

Notes on independent conjuncts:

- Wrong-kind vs constraint-invalid vs missing are three separate named tests
  (rows 8, 9, 14), each with its own kill, not one combined assertion.
- The prefix behavior is split into a positive (full id and a ≥6-char prefix
  both resolve) and a negative (short / unknown / ambiguous all fail), rows 16.
- Route parity is proven by four real routes — the person route over a PTY
  (`expectrl`), the agent stage/continue/apply route, the scripted route, and
  the crate's public library surface — not by a questions-document proxy.
- The fixture's `alpha` answers (`style=panel`, `with_tests=false`) deliberately
  differ from the template defaults (`card`, `true`) so that `beta` inheriting
  them proves seeding rather than defaulting; the seam-drop kill (row 21) makes
  every route fall back to the defaults and diverge from `expected/beta`.
