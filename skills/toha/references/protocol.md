---
relationships:
  exemplifies: interview-protocol
---

# A staged exchange

This uses a copy of `docs/examples/basic` at `$TOHA_TEMPLATE`, a fresh `$TOHA_TARGET`, and an isolated `$XDG_STATE_HOME`. The JSON lines below show selected fields from real toha output and the submitted answer document; each shell block prints exactly the following `text` block. The full batch also has `context` (canonical target and template paths, plus commit), `protocol: 1`, and `messages`.

The first call exits 4 and emits a batch with a required title. The second call submits one answers document on standard input. The next batch has a slug default and a looped array of tags.

```sh
# test
if "$TOHA_BIN" stage "$TOHA_TEMPLATE" "$TOHA_TARGET" --async > "$TOHA_TARGET/batch.json"; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys),required:.schema.required,messages}' "$TOHA_TARGET/batch.json"
if printf '%s' '{"title":"Sample Note"}' | "$TOHA_BIN" continue "$TOHA_TARGET" - > "$TOHA_TARGET/batch.json"; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys),slug_default:.schema.properties.slug.default,tags_type:.schema.properties.tags.type}' "$TOHA_TARGET/batch.json"
```

```text
{"messages":[],"questions":["title"],"required":["title"],"status":"questions"}
{"questions":["slug","tags"],"slug_default":"sample-note","status":"questions","tags_type":["array","null"]}
```

A rejected answer returns the same batch with per-id `errors` and records none of that document. Resubmit the whole batch: both `slug` and `tags` appear in the corrected document below. Then follow the remaining batches. `format` lowercases the tag in the complete answers.

```sh
# test
if printf '%s' '{"slug":"BAD NAME","tags":["One"]}' | "$TOHA_BIN" continue "$TOHA_TARGET" - > "$TOHA_TARGET/result.json"; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys),errors}' "$TOHA_TARGET/result.json"
printf '%s\n' '{"slug":"sample-note","tags":["One"]}'
if printf '%s' '{"slug":"sample-note","tags":["One"]}' | "$TOHA_BIN" continue "$TOHA_TARGET" - > "$TOHA_TARGET/result.json"; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys)}' "$TOHA_TARGET/result.json"
if printf '%s' '{"has_summary":true}' | "$TOHA_BIN" continue "$TOHA_TARGET" - > "$TOHA_TARGET/result.json"; then exit 1; else test "$?" -eq 4; fi
jq -cS '{status,questions:(.schema.properties|keys)}' "$TOHA_TARGET/result.json"
printf '%s' '{"summary":"Short text","status":"draft"}' | "$TOHA_BIN" continue "$TOHA_TARGET" - > "$TOHA_TARGET/result.json"
jq -cS '{status,answers}' "$TOHA_TARGET/result.json"
"$TOHA_BIN" apply "$TOHA_TARGET" --dry-run
```

```text
{"errors":{"slug":["validate.regex"]},"questions":["slug","tags"],"status":"questions"}
{"slug":"sample-note","tags":["One"]}
{"questions":["has_summary"],"status":"questions"}
{"questions":["status","summary"],"status":"questions"}
{"answers":{"has_summary":true,"slug":"sample-note","status":"draft","summary":"Short text","tags":["one"],"title":"Sample Note"},"status":"complete"}
create sample-note.txt
```

After reviewing the dry run, `toha apply "$TOHA_TARGET"` writes the file. A person can answer the remaining questions instead by running `toha continue "$TOHA_TARGET"` in a terminal.
