---
docs: true
title: Generating into subpaths
order: 11
relationships:
  describes: toha
  references:
    - command-line-interface
    - snapshot
    - interview-protocol
---

Some templates are meant to be applied more than once into the same project —
"add a widget", "add a module", "add a service". Each new one lives in its own
subpath and usually wants most of the same answers as the last one. Toha's
**`--like`** flag pre-fills a new application's answer defaults from a
**snapshot** of an earlier application of the same template source.

`--like` is the *generate* axis. It is a sibling of `--from` (the *update* axis),
never an overload of it: `--from` re-renders into the *same* target and merges
your edits; `--like` writes a *fresh* application into a new subpath and only
seeds its answer defaults — it never merges. The two are mutually exclusive.

## Make one, then another like it

Apply the template once into a subpath of a clean git project and commit it, so
Toha records a snapshot:

```sh
toha apply ./widget ./src/widgets/alpha --answers alpha.json
git add . && git commit -m "add alpha widget"
```

Now add a second widget, seeded from the newest one. `--like latest` selects the
newest snapshot of the same source in this repository; the scripted route's
answers document supplies only the ids you want to change, and every other
question takes its seeded default:

```sh
toha apply ./widget ./src/widgets/beta --like latest --answers beta.json
```

Each answers document uses the identity envelope
`{"template": "<canonical absolute path to widget>", "answers": {...}}`.
Copy the template identity exactly from a batch's `context.template`.

If the `answers` object in `beta.json` only sets `label`, the new `beta` inherits
`style`, `with_tests`,
and every other answer from `alpha`'s snapshot. The `applied` document carries a
`seed` member naming the snapshot it seeded from:

```json
{ "status": "applied", "seed": { "from": "01J9Z4K7QX6M2V8R0T5B3N1P9D" }, ... }
```

Because the second application also saved its own snapshot, a third can seed from
it in turn.

## Choosing the snapshot

The selector is one of:

- `latest` — the newest snapshot of the template's source.
- a snapshot id, or a prefix of at least six characters that matches exactly one
  snapshot of that source.
- omitted (`--like` with no value) — on a terminal, Toha lists the source's
  applications for you to pick one or choose "none". On the script and agent
  routes a bare `--like` is a usage error; name a selector or `latest`.

The snapshot is found repository-wide and filtered by *source identity* — the
template's formal name without its `@reference` — so a prior application at a
different subpath seeds the new one, and a snapshot of a different source is
refused. A version bump of the same source still seeds; a foreign source does
not.

## Overriding and mismatches

A seeded value is a *default*, not a submitted answer. At a terminal you accept
or type over each one; with `--answers` any id you supply wins over its seed. If
the newer template version changed a question's type so a seeded value no longer
fits, Toha drops that value with a warning and falls back to the configured or
template default. If a seeded value still violates a tightened constraint, Toha
re-asks it (at a terminal) or returns it as a remaining question (exit code 4) —
it is never written.

The per-answer precedence, highest first, is: an answer the route submits, then a
kind-matching snapshot value, then a configured preset or template default, then
the template's own default, then the question is asked.

## Staging and resuming

An agent stages a seeded interview and drives it step by step. The stage pins the
chosen snapshot id, so `continue` and `apply PATH` rebuild the identical seeded
defaults even if a newer snapshot appears in between:

```sh
toha stage ./widget ./src/widgets/gamma --like latest --async
toha continue ./src/widgets/gamma answers.json
toha apply ./src/widgets/gamma
```

## When there is nothing to seed from

Omitting `--like` always applies normally. An explicit `--like` that cannot be
satisfied — no snapshot of the source, an unknown or ambiguous reference, a
snapshot of another source, or a target that is not in a git repository — fails
with an `error` document of kind `snapshot` (exit code 1) rather than silently
applying without a seed.
