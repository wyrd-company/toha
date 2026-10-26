---
title: Generating into subpaths
order: 14
product: toha
relationships:
  describes: toha
  references:
    - command-line-interface
    - applied-record
    - interview-protocol
---

Some templates are meant to be applied more than once into the same project —
"add a widget", "add a module", "add a service". Each new one lives in its own
subpath and usually wants most of the same answers as the last one. Toha's
**`--like`** flag pre-fills a new application's answer defaults from the
**applied record** of an earlier application of the same template source.

`--like` is the *generate* axis. It is a sibling of `--update` (the *update* axis),
never an overload of it: `--update` re-renders into the *same* target and merges
your edits; `--like` writes a *fresh* application into a new subpath and only
seeds its answer defaults — it never merges. The two are mutually exclusive.

## Make one, then another like it

Apply the template once into a subpath of a clean git project and commit it, so
Toha records the application:

```sh
toha apply ./widget ./src/widgets/alpha --answers alpha.json
git add . && git commit -m "add alpha widget"
```

Add a second widget, seeded from the first application's target. The scripted
route supplies only the answers you want to change:

```sh
toha apply ./widget ./src/widgets/beta --like src/widgets/alpha --answers beta.json
```

Each answers document uses the identity envelope
`{"template": "<canonical absolute path to widget>", "answers": {...}}`.
Copy the template identity exactly from a batch's `context.template`.

If the `answers` object in `beta.json` only sets `label`, the new `beta` inherits
`style`, `with_tests`,
and every other answer from `alpha`'s entry. The `applied` document's
`seed` member names the target it seeded from. Its value is:

```json
{ "from": "src/widgets/alpha" }
```

The second application has its own entry in `.toha.json`, so a third can seed
from `src/widgets/beta`.

## Choosing the application

Name the repository-relative target of an application of the same source.
The source is the formal name without its reference; a version of the same
source can seed another version. A bare `--like` in a terminal lists applications
to choose from. Script and agent routes require a target.

## Overriding and mismatches

A seeded value is a *default*, not a submitted answer. At a terminal you accept
or type over each one; with `--answers` any id you supply wins over its seed. If
the newer template version changed a question's type so a seeded value no longer
fits, Toha drops that value with a warning and falls back to the configured or
template default. If a seeded value still violates a tightened constraint, Toha
re-asks it (at a terminal) or returns it as a remaining question (exit code 4) —
it is never written.

The per-answer precedence, highest first, is: an answer the route submits, then
a kind-matching recorded value, then a configured preset or template default,
then the template's own default, then the question is asked.

## Staging and resuming

An agent stages a seeded interview and drives it step by step. Staging copies
the chosen entry's answers and parts, so later changes to the entry do not change
the staged defaults:

```sh
toha stage ./widget ./src/widgets/gamma --like src/widgets/beta --async
toha continue ./src/widgets/gamma answers.json
toha apply ./src/widgets/gamma
```

## When there is nothing to seed from

Omitting `--like` applies normally. A target without a matching application of
the source, or a target outside a Git repository, fails with an `error` document
of kind `record` at exit code 1.
