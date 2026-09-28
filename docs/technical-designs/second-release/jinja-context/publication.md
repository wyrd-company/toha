---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - error-attribution
    - jinja-includes
---

# Jinja context design publication

- Postplan URL: <https://t30jlda1nnbd.postplan.dev>
- Draft ID: `t30jlda1nnbd`
- Version: 8
- Exact design content revision (SHA-256):
  `2711043ef216d965bd05c8d8e477f2f0f4d2bca020b936c7f405d06b537565e9`
- Accepted producer integration revision:
  `8a19269d78f6da0738edb17e56310621f143923d`
- Current design review base and merge-base:
  `d33245065f80e3ed9a8c75ea6526e8a9f9386f21`
- Portable source: `brief.source.html`
- Built deck: `brief.deck.html`
- Design SHA-256: `2711043ef216d965bd05c8d8e477f2f0f4d2bca020b936c7f405d06b537565e9`
- Design YAML SHA-256: `bd7b527417928b5cc25a15ade0b79ea9e4ca3856d4e412ed429660bf28c5ab89`
- Verification SHA-256: `c8c7a7fa8f259edcf138305d08bcf00db2b89efecee22f6f22976633367d17df`
- Rationale SHA-256: `52b841c7b2668e7cf818c36c58e9cc899e8ac1a8542a7790828a8321e7ab27c7`
- Producer composition re-ground SHA-256: `be4052ca8de4df760ee72f33806d69dbddfee3f258e1e1093563710dcb5be740`
- Stage-admission synthesis SHA-256: `cbd29527c079427d452e438408885f010604395761413605bc164b730da71b70`
- Brief source SHA-256: `f43441995c4c3783723b068971fe58b000bf7502902c712e6b1e7982c96cd1ac`
- Built and published raw SHA-256: `fc4e4e33826d9416d70e703c8d9266d73f4f2e86c934100d8d1dd4f5ce77d360`
- Built and published raw bytes: 321210
- Published `Reveal.initialize` count: 1
- Published horizontal positions: 14
- Published slide count: 36 (all horizontal and vertical positions)

This revision composes the separately approved Jinja-context decision with the
accepted producer implementation. Bob's error-attribution approval binds the
producer's original checkpoint `067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`,
design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`, all
recommendations including choices 1A–5A, and the separately disclosed removal
of raw-path inputs from identity-sensitive consumers. The accepted producer
factory is `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>`;
`as_path()` is its only projection. There is no `TargetError`, unchecked
constructor, or second normalizer.

The actual base contains origin-bearing configured `Resolution`, private
`ResolvedDefault { raw, origin }`, and `DefaultBankEntry::Configured`. The
design's context-bearing configured start consumes that resolution through
`start_with_context`; configured replay keeps `replay_with_resolution`, which
restores context inside `StagedRecord` from the stored wire and producer
carrier. Only the distinct ordinary flat `Seed` route uses flat defaults.
`ConfigEntry` remains the sole origin source, and
`into_flat_defaults()` remains an explicit provenance-dropping escape.

This producer approval does not expand Bob's earlier Jinja-context approval.
The four-case stage matrix, five named optional values, fixed-five snapshot,
no environment read without an explicit carried stage grant, replay without a
trust flag or live recheck, and approved mutable-folder behavior remain under
that context approval. No new context decision is requested by this producer
alignment.

## Composition evidence

The exact base source was inspected in `src/config.rs`, `src/interview.rs`,
`src/staging.rs`, `src/main.rs`, `src/plan.rs`, and `src/protocol.rs`. The
source-to-consumer chain, signatures, warning order, failure attribution, and
caller evidence are in
[producer-composition-reground.md](producer-composition-reground.md).

The Jinja context uses the four-case stage matrix. No references and no flag
records unavailable with zero reads. An explicit `stage --trust` captures the
fixed five once even without an initial reference. References without the flag
fail before interview or staged-state progress. References with the flag
capture the fixed five once. Continue has no trust option, live environment
read, recheck, or access abort/retry. Mutable folders use the recorded result
without a source or program identity refusal.

The configured-presets contract remains answer data, not Jinja context.
`ConfigEntry` remains the sole origin source. The stage adapter maps only the
typed environment trust refusal to `StagingError::EnvironmentTrustRequired`;
other admission faults retain their source and evaluator detail.

The public `StagedRecord::replay`, `replay_with_defaults`, and
`replay_with_resolution` routes remain available to external crate callers.
Each has an explicit signature in `design.md`, takes the producer-created
target carrier, and restores current or legacy context from the staged record;
callers do not build private context modes or capture live environment values
during replay. The existing `new` constructor remains available for
legacy-compatible records; current records use `new_with_context`.

## Validation and rendered deck

The exact-head task gates, full artifact checks, independent review basis,
Postplan raw-byte equality, and per-slide visual/bounds results are recorded in
the task handoff. The deck is rebuilt from `brief.source.html`; the committed
deck and published response are byte-identical. Every horizontal and vertical
slide was rendered after a 700 ms transition settle and checked at 1280×720 for
slide and visible-descendant bounds; all 36 had zero bounds violations. All 36
renders were visually scanned in a contact sheet. The new side-by-side public
replay slide was inspected at full resolution. Manager-facing slides use plain
language, side-by-side before/after examples, and vertical stacks for long
decisions and lists. Technical detail stays in a short appendix.

This design task changes no runtime or shared canonical artifact. The paired
implementation owns those changes after review.
