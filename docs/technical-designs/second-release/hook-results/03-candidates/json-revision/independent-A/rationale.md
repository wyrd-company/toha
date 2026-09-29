# Rationale — candidate A

Lens: minimize new surface. The parsed value should *be* `<id>`. Metadata is
secondary, and the common case (read one JSON field) needs one extra line of
YAML.

## Why this reconciliation

The hard question is where `exit_code`, raw `stdout`, and `stderr` go when
`<id>` is arbitrary JSON. A value that is arbitrary JSON cannot also carry a
fixed key without a collision risk. It also cannot carry metadata at all when
it is an array or a scalar. So metadata needs a **different name**. There are
three ways to get one:

| Way | Collision | New surface | Verdict |
| --- | --- | --- | --- |
| Fixed companion name (for example a reserved `hooks.<id>` or a derived `<id>_status`) | none | a new reserved name, or derived names that silently enter the authored-id space | rejected: it undoes v1's "no new reserved name", and derived names hide a collision until someone names a question `pkg_status` |
| Author-named companion (`status-id: <name>`) | none; the existing duplicate/reserved checks apply | one optional field | **chosen** |
| No metadata in JSON mode at all | none | zero | rejected: it loses tolerated failure. Lint tools print JSON findings *and* exit nonzero, which is the main reason to combine the two features |

The author-named companion keeps both halves honest:

- `<id>` is exactly the JSON value, with no reserved keys, no wrapper, and no
  special attribute. `{{ pkg }}` for a string is the string. `pkg.exit_code` is
  the JSON's own key.
- `<status-id>` is the v1 result shape minus stdout. It reuses v1's
  capture/read rules, "exit_code none ⇔ did not run", and the `allow-failure`
  rule. No new metadata semantics are invented.
- Most JSON hooks never declare `status-id`, because a default-mode producer
  succeeded by construction when it is read. v1 already makes this argument for
  `version.stdout`.

## Other choices

- **`parse: json` implies stdout capture.** `capture: [stdout]` plus
  `parse: json` would say the same thing twice. The redundant form is a load
  error that names the fix, so there is one way to write it. `capture:
  [stderr]` stays the v1 knob for stderr.
- **Raw stdout is not exposed in JSON mode.** `<id> | tojson` covers the
  real use (pass the document on). Exposing raw text too would put two sources
  of truth for one stream on the reading surface.
- **Did not run = undefined, not `none`.** JSON `null` is a legitimate output,
  and v1's "none means did not run" would conflict with it. Undefined is
  Jinja-native (`is defined`) and costs no new name. It also fails loudly when
  someone reads through an unguarded conditional producer (the lenient mode
  rejects an attribute read through undefined), where the alternative is a
  silent empty value.
- **Strict on success, lenient on tolerated failure.** A successful JSON-mode
  hook that prints non-JSON breaks its contract, so it stops the apply, as v1
  does for non-UTF-8. A failure that the author already chose to tolerate should
  not become fatal because its output is junk. `status-id` (required with
  `allow-failure`) tells the reader why `<id>` is undefined.
- **Non-UTF-8 stays fatal in every case.** This is the v1 decode policy, which
  applies before any parse.
- **Untolerated failure: nothing is parsed.** The apply is already failing. A
  parse error would only hide the real `ApplyError::Hook`.
- **Empty stdout on success is an error, not `none`.** Empty text is not JSON.
  Mapping it to `none` would make "printed nothing" look the same as "printed
  null".

## Tradeoffs and risks

- **Two names for one hook in the failure case.** `findings` and `lint` must
  both be read. This is the cost of zero collision. It applies only when
  `allow-failure` or `stderr` is used.
- **Undefined departs from v1 invariant 3** ("never undefined") for the JSON
  value only. The status object keeps invariant 3.
- **Render error after files are written** if a reader dereferences an
  unguarded, unrun JSON producer. This is the same failure class as v1's
  deferred render fault. A load-time lint for "conditional producer read without
  `is defined`" is possible later, but it is not proposed here.
- **Number precision.** serde_json without `arbitrary_precision` reads large
  integers above u64 as f64. Enabling that feature was considered and rejected,
  so that the dependency stays as it is.
- **Assumption:** the minijinja lenient undefined behavior stays the Toha default
  (`src/jinja.rs` sets no other behavior). The guard semantics depend on it.
- **No size limit on captured stdout**, the same as v1. Adding a limit would be a
  new limit mechanic that needs Bob's approval.

## Synthesis decision
