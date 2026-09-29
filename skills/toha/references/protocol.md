---
relationships:
  exemplifies: interview-protocol
---

# A staged exchange

This uses a copy of the template below at `$TOHA_TEMPLATE`, a fresh `$TOHA_TARGET`, and isolated configuration, data, cache, and state directories. `TOHA_BIN`, `TOHA_TEMPLATE`, and `TOHA_TARGET` are shell variables of this exchange; toha does not read them. `toha skills view toha --path references/environment.md` explains the variables that isolate the run.

`template.yml`:

```yaml
name: basic
description: A note file with a title, tags and an optional summary

interview:
  - id: title
    type: text
    prompt: Title
    required: true
    validate:
      min: 3
      max: 80

  - id: slug
    type: text
    prompt: File name
    default: "{{ title | kebab }}"            # template: text default is a string
    validate:
      regex: '^[a-z0-9-]+$'

  - id: tags
    type: text
    prompt: "Tag (empty to finish)"
    loop:                                       # answer is an array of strings
      min: 0
      max: 5
    format: value | lower                       # expression, applied to each item

  - id: has_summary
    type: confirm
    prompt: Add a summary?
    default: "tags | length > 0"                # expression: confirm default is a bool

  - id: summary
    type: multiline
    prompt: Summary
    when: has_summary

  - id: status
    type: select
    prompt: Status
    options: [ draft, review, final ]             # literal array
    default: draft                              # template: select default is a string
```

`template/{{ slug }}.txt`:

```jinja
# {{ title }}
Status: {{ status }}
Tags: {{ tags | join(', ') }}
{% if has_summary %}{{ summary }}
{% endif %}
```

This is the agent route: `stage TEMPLATE PATH --async [FILE]`, then
`continue PATH FILE` (with `-` for standard input), then `apply PATH`. Every
answers document names the template it answers with the two-field envelope
`{"template": <formal>, "answers": {...}}`, copying the formal name the batch
publishes as `context.template`. `stage --async FILE` writes the batch to the
file and the instructions to standard output; `continue PATH -` leads standard
output with the batch JSON followed by plain-text instructions, so the exchange
below reads the leading JSON document with `sed '/^}$/q'`. The full batch also has
`context` (canonical target and template paths, and the commit of a git template
or `null` for a folder), `protocol: 1`, and `messages`. Each shell block prints
exactly the following `text` block.

The first call exits 4 and writes a batch with a required title. Each
`continue PATH -` submits one answers document; a rejected answer returns the
same batch with per-id `errors` and records nothing. The optional `status`
select lists its options in `anyOf`, beside `null`. `format` lowercases each tag.
The completing `continue` writes instructions only and exits 0, and
`apply PATH --dry-run` previews the file plan without writing.

```sh
# test
export HOME="$TOHA_TARGET/home"
export XDG_CONFIG_HOME="$TOHA_TARGET/config"
export XDG_DATA_HOME="$TOHA_TARGET/data"
export XDG_CACHE_HOME="$TOHA_TARGET/cache"
export XDG_STATE_HOME="$TOHA_TARGET/state"
export TOHA_USER_CONFIG="$XDG_CONFIG_HOME/toha/config.yml"
export TOHA_CONFIG="$TOHA_TARGET/local.yml"
peek() { sed '/^}$/q' "$1"; }
if "$TOHA_BIN" stage "$TOHA_TEMPLATE" "$TOHA_TARGET" --async "$TOHA_TARGET/batch.json" >/dev/null; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys),required:.schema.required,messages}' "$TOHA_TARGET/batch.json"
formal=$(jq -r .context.template "$TOHA_TARGET/batch.json")
answer() { printf '{"template":"%s","answers":%s}' "$formal" "$1"; }
if answer '{"title":"Sample Note"}' | "$TOHA_BIN" continue "$TOHA_TARGET" - >"$TOHA_TARGET/out.txt"; then exit 1; else test "$?" -eq 4; fi
peek "$TOHA_TARGET/out.txt" | jq -cS '{status,questions:(.schema.properties|keys),slug_default:.schema.properties.slug.default,tags_type:.schema.properties.tags.type}'
if answer '{"slug":"BAD NAME","tags":["One"]}' | "$TOHA_BIN" continue "$TOHA_TARGET" - >"$TOHA_TARGET/out.txt"; then exit 1; else test "$?" -eq 4; fi
peek "$TOHA_TARGET/out.txt" | jq -cS '{status,questions:(.schema.properties|keys),errors}'
if answer '{"slug":"sample-note","tags":["One"]}' | "$TOHA_BIN" continue "$TOHA_TARGET" - >"$TOHA_TARGET/out.txt"; then exit 1; else test "$?" -eq 4; fi
peek "$TOHA_TARGET/out.txt" | jq -cS '{status,questions:(.schema.properties|keys)}'
if answer '{"has_summary":true}' | "$TOHA_BIN" continue "$TOHA_TARGET" - >"$TOHA_TARGET/out.txt"; then exit 1; else test "$?" -eq 4; fi
peek "$TOHA_TARGET/out.txt" | jq -cS '{status,questions:(.schema.properties|keys),status_options:.schema.properties.status.anyOf}'
answer '{"summary":"Short text","status":"draft"}' | "$TOHA_BIN" continue "$TOHA_TARGET" - >/dev/null
"$TOHA_BIN" apply "$TOHA_TARGET" --dry-run
```

```text
{"messages":[],"questions":["title"],"required":["title"],"status":"questions"}
{"questions":["slug","tags"],"slug_default":"sample-note","status":"questions","tags_type":["array","null"]}
{"errors":{"slug":["must match ^[a-z0-9-]+$"]},"questions":["slug","tags"],"status":"questions"}
{"questions":["has_summary"],"status":"questions"}
{"questions":["status","summary"],"status":"questions","status_options":[{"enum":["draft","review","final"]},{"type":"null"}]}
create sample-note.txt
```

After reviewing the dry run, `toha apply "$TOHA_TARGET"` writes the file. A person
can answer the remaining questions instead by running
`toha continue "$TOHA_TARGET"` in a terminal.
