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
- Version: 7
- Exact design content revision (SHA-256):
  `4627b6e93e1170f74025b1d191f548354a7bd2a18a0e69f9ddc202613389a73c`
- Accepted producer integration revision:
  `8a19269d78f6da0738edb17e56310621f143923d`
- Current design review base and merge-base:
  `d33245065f80e3ed9a8c75ea6526e8a9f9386f21`
- Portable source: `brief.source.html`
- Built deck: `brief.deck.html`
- Design SHA-256: `4627b6e93e1170f74025b1d191f548354a7bd2a18a0e69f9ddc202613389a73c`
- Design YAML SHA-256: `db72b87d054a4aa97fee59fd71884ce5a9503d9fe4ee7bda657b146e4a4285b7`
- Verification SHA-256: `7df29ddf863a541d3436f5c0f898fb0e89068b03f8c80b1028cfbac2f8e46c7e`
- Rationale SHA-256: `50f96cbbc32902c76d7358ba3e521363a8afe824ac6a1d85d70f678acae86896`
- Producer composition re-ground SHA-256: `f78af96ae4697c324340dde06164b348159b1e5d61118ed76f96728bdf733086`
- Brief source SHA-256: `7f87c3ee127ce1629e1bc7e9db731b72b30c0fe9eb2a6c0ce8588d9d4104ad0f`
- Built and published raw SHA-256: `02357d17140b4ac210143e10bdb295cf13318fd39c9db17df1ee7580f454f435`
- Built and published raw bytes: 319927
- Published `Reveal.initialize` count: 1
- Published horizontal positions: 14
- Published slide count: 35 (all horizontal and vertical positions)

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
`start_with_context`; configured replay keeps `replay_with_resolution` and
receives the restored context. Only the distinct ordinary flat `Seed` route
uses flat defaults. `ConfigEntry` remains the sole origin source, and
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

## Validation and rendered deck

The exact-head task gates, full artifact checks, independent review basis,
Postplan raw-byte equality, and per-slide visual/bounds results are recorded in
the task handoff. The deck is rebuilt from `brief.source.html`; the committed
deck and published response are byte-identical. Every horizontal and vertical
slide was rendered after a 650 ms transition settle and checked at 1280×720 for
viewport, body, section, and visible-descendant bounds; all 35 had zero bounds
violations. All 35 renders were visually scanned in a contact sheet. The changed
target-interface slide was inspected at full resolution. Manager-facing slides
use plain language, side-by-side before/after examples, and vertical stacks for
long decisions and lists. Technical detail stays in a short appendix.

This design task changes no runtime or shared canonical artifact. The paired
implementation owns those changes after review.
