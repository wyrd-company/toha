# Serde ecosystem vs. in-place JSON-family editing

Research question: can serde-based crates edit JSON/JSONC/JSON5 files in
place while preserving comments, key order, and formatting?

Repos cloned (shallow, `--depth 1`) into `/workspaces/references/toha/`:

| Crate | Repo | Cloned HEAD | Commit date | Cargo.toml version |
| --- | --- | --- | --- | --- |
| serde_json | https://github.com/serde-rs/json | `afdf6fc67247dd7fa4fcde1381e6ecc6bcc7a30e` | 2026-08-07 23:50:03 -0700 | 1.0.151 |
| json5 (json5-rs) | https://github.com/callum-oakley/json5-rs | `fd55f902db05e47418016eb02593f66a41ec087c` | 2026-09-26 10:40:07 +0100 | 1.3.1 |

---

## serde_json — verdict: RESERIALIZES (always)

### `preserve_order` feature = insertion order, not "original file order" guarantee

`Cargo.toml` feature comment (serde_json, `Cargo.toml`):

```
# Make serde_json::Map use a representation which maintains insertion order.
# This allows data to be read into a Value and written back to a JSON string
# while preserving the order of map keys in the input.
preserve_order = ["indexmap", "std"]
```

`src/map.rs:1-36`:

```rust
1  //! A map of String to serde_json::Value.
2  //!
3  //! By default the map is backed by a [`BTreeMap`]. Enable the `preserve_order`
4  //! feature of serde_json to use [`IndexMap`] instead.
...
23 #[cfg(not(feature = "preserve_order"))]
24 use alloc::collections::{btree_map, BTreeMap};
25 #[cfg(feature = "preserve_order")]
26 use indexmap::IndexMap;
...
29 pub struct Map<K, V> {
30     map: MapImpl<K, V>,
31 }
32
33 #[cfg(not(feature = "preserve_order"))]
34 type MapImpl<K, V> = BTreeMap<K, V>;
35 #[cfg(feature = "preserve_order")]
36 type MapImpl<K, V> = IndexMap<K, V>;
```

Without `preserve_order`, `Value::Object` is a `BTreeMap` — keys always come
back out **alphabetically sorted**, unrelated to the source file. With
`preserve_order`, it's an `indexmap::IndexMap`, which preserves **insertion
order** — i.e. the order keys were first inserted during a straight parse,
which does coincide with source order for an unmodified round-trip parse →
serialize. But it is not a durable link to "the original file's order": any
programmatic edit changes it per ordinary `IndexMap` semantics —
`insert` of an existing key updates in place (order unchanged), but
`.remove()` documented as shifting via `swap_remove` (`src/map.rs:150-171`,
"If serde_json's `preserve_order` is enabled, `.remove(key)` is equivalent to
[`Map::swap_remove`]"), and a freshly-inserted new key is appended at the
end, not spliced into its "logical" position in the source. There's also no
concept of preserving order for keys the caller never touched but that
existed in a partially-reconstructed `Value` (e.g. built via `json!{}` or by
`Deserialize`-ing into a Rust struct and re-serializing) — struct field order
becomes struct declaration order, not file order, since structs don't route
through `Map` at all.

### No comment or trailing-comma support in the parser

`src/de.rs` whitespace skipper only understands JSON whitespace bytes, not
`/`:

```rust
255    fn parse_whitespace(&mut self) -> Result<Option<u8>> {
256        loop {
257            match tri!(self.peek()) {
258                Some(b' ' | b'\n' | b'\t' | b'\r') => {
259                    self.eat_char();
260                }
261                other => {
262                    return Ok(other);
263                }
264            }
265        }
266    }
```

A `/` (comment start) falls through to `other` and is handled as if it were
the start of a value/token, producing a parse error — there is no comment
production anywhere in `src/de.rs` or `src/read.rs`.

Trailing commas are explicitly rejected with a dedicated error code,
`ErrorCode::TrailingComma` (`src/error.rs:301`, message at `src/error.rs:381`
`"trailing comma"`), raised at four call sites in `src/de.rs`:

- `src/de.rs:1081` — `Ok(Some(b']')) => Err(self.peek_error(ErrorCode::TrailingComma))`
- `src/de.rs:1096` — `Some(b',') => Err(self.peek_error(ErrorCode::TrailingComma))`
- `src/de.rs:1955` — same pattern for `Seq` visitor path
- `src/de.rs:2011` — same pattern for `Map` visitor path

So serde_json is strict-JSON only: no comments, no trailing commas, no
JSON5/JSONC syntax.

### Pretty formatter always reformats; indentation is configurable but nothing else is preserved

`src/ser.rs:1945-1965`:

```rust
1945 pub struct PrettyFormatter<'a> {
1946     current_indent: usize,
1947     has_value: bool,
1948     indent: &'a [u8],
1949 }
1951 impl<'a> PrettyFormatter<'a> {
1952     /// Construct a pretty printer formatter that defaults to using two spaces for indentation.
1953     pub fn new() -> Self {
1954         PrettyFormatter::with_indent(b"  ")
1955     }
1956
1957     /// Construct a pretty printer formatter that uses the `indent` string for indentation.
1958     pub fn with_indent(indent: &'a [u8]) -> Self {
1959         PrettyFormatter {
1960             current_indent: 0,
1961             has_value: false,
1962             indent,
1963         }
1964     }
1965 }
```

The only knob is the indent byte string (`with_indent`); everything else —
newline placement, spacing around `:`/`,`, brace style — is fixed by the
`Formatter` trait callbacks (`begin_array`, `end_array`,
`begin_array_value`, `begin_object`, etc., `src/ser.rs:1973+`), which are
driven purely by the shape of the `Value`/`Serialize` tree being walked, with
no input to reference. `to_string_pretty`, `to_writer_pretty`,
`to_vec_pretty` (`src/ser.rs:2197`, `2229`, `2264`) all construct output from
scratch this way. There is no "diff against original bytes" or
whitespace-preservation mode anywhere in `ser.rs`.

### `Value` carries zero formatting/comment metadata

`src/value/mod.rs:116-133` — the `Value` enum:

```rust
116 pub enum Value {
117     /// Represents a JSON null value.
124     Null,
126     /// Represents a JSON boolean.
133     Bool(bool),
135     /// Represents a JSON number, whether integer or floating point.
...     Number(Number),
...     String(String),
...     Array(Vec<Value>),
...     Object(Map<String, Value>),
136 }
```

Six variants, all pure data — no span, no trivia, no raw-text slot. (The
`raw_value` feature's `RawValue` type lets you defer parsing of a subtree and
re-emit its *original bytes verbatim as a single opaque blob*, but that only
works for a whole value you never inspect/mutate; it cannot be edited and
still preserve formatting, and it does not help with comments living
alongside real, model-visible keys.)

### Net verdict

serde_json is fundamentally a **RESERIALIZE** approach for any edit
task:

- Comments: always lost (parser has no production for them; strict-JSON
  only, confirmed via `parse_whitespace` and absence of any comment
  handling in `de.rs`/`read.rs`).
- Key order: lost by default (`BTreeMap`, alphabetical); only
  *insertion-order-preserving*, not source-fidelity-preserving, with
  `preserve_order` (`indexmap::IndexMap`, `src/map.rs:26,36`) — good enough
  for untouched round-trips, not a guarantee once a `Value` is edited,
  merged, or rebuilt via structs.
  reformats using its own indent rules (`src/ser.rs:1945-2048`); no whitespace
  or layout survives.

---

## json5 / json5-rs — verdict: PARSE-ONLY (comments discarded on parse; no round-trip preservation)

### Parses full JSON5 grammar, including comments and trailing commas

Doc comment in `src/lib.rs` states the crate's scope directly:

```
//! [JSON5][] is a superset of [JSON][] with an expanded syntax including some productions from
//! [ECMAScript 5.1][]. ... In particular, JSON5 allows comments, trailing commas, object keys
//! without quotes, single quoted strings, hexadecimal numbers, multi-line strings...
```

`src/de.rs:138-178` — `skip_whitespace` / `skip_comment`:

```rust
138    fn skip_whitespace(&mut self) -> Result<()> {
139        while let Some((_, c)) = self.peek() {
140            match c {
141                _ if crate::char::is_json5_whitespace(c) => {
142                    self.next();
143                }
144                '/' => {
145                    self.next();
146                    self.skip_comment()?;
147                }
148                _ => {
149                    break;
150                }
151            }
152        }
153        Ok(())
154    }
155
156    // https://spec.json5.org/#comments
157    fn skip_comment(&mut self) -> Result<()> {
158        let (offset, c) = self.next_or(ErrorCode::EofParsingComment)?;
159        match c {
160            '/' => {
161                while let Some((_, c)) = self.next() {
162                    if crate::char::is_json5_line_terminator(c) {
163                        break;
164                    }
165                }
166            }
167            '*' => {
168                while let Some((_, c)) = self.next() {
169                    if c == '*' && self.peek().is_some_and(|(_, c)| c == '/') {
170                        self.next();
171                        break;
172                    }
173                }
174            }
175            _ => {
176                return Err(self.err_at(offset, ErrorCode::ExpectedComment));
177            }
178        }
```

`skip_comment` consumes and **discards** comment bytes; nothing captures
them into any structure — they are simply advanced-past, exactly like
whitespace. There is no comment/trivia type anywhere in the crate (confirmed
by `rg` across `src/`: no `Comment` struct, no span table).

Unquoted object keys are parsed by a dedicated `parse_key`
(`src/de.rs:457`), and trailing commas are explicitly demonstrated as
supported grammar in the crate's own doc example
(`src/lib.rs`: `trailingComma: 'in objects', andIn: ['arrays',],`).

### No `Value` type, no formatting-aware serializer — pure serde in/out

`rg -n "pub enum Value|pub struct Value" src/` returns nothing: json5-rs
does not define its own DOM/AST type. `from_str` (`src/de.rs:36-43`) drives a
`serde::Deserialize` visitor directly off the token stream — there is no
intermediate tree a caller could hold onto and mutate before re-emitting.

Serialization is symmetric and equally uninformed by any prior input:
`src/ser.rs:34` `pub fn to_string<T: Serialize>(...)` and `src/ser.rs:46`
`pub fn to_writer<T: Serialize, W: Write>(...)` walk a `Serializer` struct
(`src/ser.rs:51`) driven purely by the `Serialize` impl of the Rust value
being written — same class of formatter-from-shape approach as serde_json's
`PrettyFormatter`, with no channel for reinjecting original comments or
layout.

(Dev-dependency on `serde_json` and `indexmap` in `Cargo.toml` is test/bench
tooling only — e.g. comparing against `serde_json5` — not a runtime feature
of the crate.)

### Net verdict

json5-rs is **PARSE-ONLY**: it accepts a strictly larger, more permissive
grammar than serde_json (comments, trailing commas, unquoted keys, etc.) but
throws all of that extra syntax away during parsing. It gives you a plain
Rust value via `serde::Deserialize` with zero residue of comments or
formatting, and its serializer round-trips through that same
formatting-blind path. Re-emitting after any edit reformats from scratch and
drops every comment in the source — no better than serde_json for
preservation, just a wider *input* grammar.

---

## Why no serde-based crate can preserve comments in general

Not a repo claim — an implication of what's in both crates above. Serde's
core contract is the `Serialize`/`Deserialize` **data model**: a fixed set of
self-describing primitives (bool, integers, floats, char, string, byte
array, option, unit, unit_struct, unit_variant, newtype variants, seq, tuple,
map, struct, enum) that every format crate must translate to and from. There
is no slot in that model for "trivia" (comments, exact whitespace, key
ordering as authored) — `serde_json::Value` (`src/value/mod.rs:116-136`,
cited above) and json5-rs's total absence of any DOM type are two concrete
demonstrations of the same fact: whatever a serde `Deserializer` extracts
must fit into that closed primitive set, and whatever a serde `Serializer`
emits is generated purely from that set with no back-channel to the
original bytes. Any crate that *does* preserve comments/formatting
(e.g. `toml_edit`, `jsonc-parser`-style CST editors, `taplo`) necessarily
works by NOT deserializing through `serde::Deserialize`/`Serialize` for the
edit path — it maintains its own concrete/lossless syntax tree and only
offers a serde bridge, if any, as a lossy convenience view on top. That's
outside the two crates researched here and was not cloned per task scope.

---

## Summary table

| crate | parses comments | preserves comments on emit | preserves key order | preserves whitespace | notes |
| --- | --- | --- | --- | --- | --- |
| serde_json 1.0.151 | No (strict JSON; `/` is a parse error via `parse_whitespace`, `src/de.rs:255-266`) | No — no comment production exists | No by default (`BTreeMap`, alphabetical, `src/map.rs:34`); insertion-order only with `preserve_order` feature (`IndexMap`, `src/map.rs:36`) — not a source-order guarantee once edited | No — `PrettyFormatter` always regenerates layout from the `Value` shape (`src/ser.rs:1945-2048`); only the indent string is configurable | Also rejects trailing commas (`ErrorCode::TrailingComma`, `src/de.rs:1081,1096,1955,2011`) |
| json5 1.3.1 (json5-rs) | Yes (`skip_comment`, `src/de.rs:156-178`, handles `//` and `/* */`) | No — comments are consumed and dropped, never captured (`src/de.rs:138-154`) | N/A — crate has no `Value`/map type of its own (`rg` for `pub enum/struct Value` in `src/` returns nothing); order is whatever the target Rust type's `Serialize` impl produces | No — `Serializer` in `src/ser.rs:34-51` builds output purely from the `Serialize` impl, no link to source bytes | Also parses trailing commas and unquoted keys (`src/lib.rs` doc example; `parse_key`, `src/de.rs:457`) but discards that extra syntax the same way |

Both crates fall on the "closed serde data model" side: whatever leaves a
`Deserializer` and enters a `Serializer` must fit primitives with no trivia
slot, so both are reserialize-from-scratch on emit. json5-rs has a strictly
richer *parser* (accepts JSON5/JSONC-flavored syntax), serde_json has a
config knob for map ordering — neither changes the fundamental answer for
Toha's content-injection use case: neither crate can edit a JSON-family file
in place while preserving comments, source key order, or original
formatting.
