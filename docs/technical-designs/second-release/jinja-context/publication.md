---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - error-attribution
    - jinja-includes
---

# Jinja context approved design publication

- Postplan URL: <https://t30jlda1nnbd.postplan.dev>
- Draft ID: `t30jlda1nnbd`
- Version: 4
- Approved design revision:
  `521b2677e52bea9f8f99751faa5788c9699aa8d6`
- Epic base and merge-base:
  `dfe7ba017ebef525310db8b8ab4ead58fae2d147`
- Portable source: `brief.source.html`
- Built deck: `brief.deck.html`
- Design SHA-256:
  `ab5397c8cf87ce40c3b02d92e8c555541119a3e335145d3fd87eaecd8d0f699e`
- Verification SHA-256:
  `85dc04e0fbeb8ace6ec3027145f22380279888c54e4bb2eb2961c659aebe8ec2`
- Brief source SHA-256:
  `747aa32489096082d30f59beeb2b4dcda2ef6bfdeda5d716589dbfdf9f570b23`
- Built and published raw SHA-256:
  `db36ddf4035a241989e8615ed6d7f531ea772c344316f2bf2319948f22496348`
- Built and published raw bytes: 318401
- Published `Reveal.initialize` count: 1
- Published horizontal positions: 14
- Published slide count: 34

The committed source rebuilds byte-for-byte to the committed deck. The local
deck, published raw response, and published deck response are byte-identical.
All 34 horizontal and vertical slides were rendered after the Reveal transition
settled and inspected at 1280×720. Programmatic bounds checks found no viewport,
document-body, slide-section, or visible-descendant overflow. Dense seam,
interface, trust, data, replay, analysis, arena, and closure slides were also
inspected at original resolution. No slide clips or requires scrolling.

The approved design uses the four-case stage matrix. A stage without references
or `--trust` records `Unavailable` with zero environment reads. Explicit
`stage --trust` captures the fixed five once even without an initial reference.
A stage with references but no flag fails before interview or staged-state
progress. References with the flag capture the fixed five once. Continue has no
trust flag, live environment read, recheck, or access abort/retry. Mutable
folders use the recorded result without a source or program identity refusal.

The exact epic base contains configured presets, the hook-review contract, the
Jinja-include design, and the canonical-target producer. Presets remain answer
defaults and are not Jinja context. Configured start and replay keep winning
`ConfigEntry` origins; ordinary direct callers keep flat `Seed.defaults`.

The include binding is revision
`b87a5482b97e81524272bee1e526698c862754db`, final-design SHA-256
`3c996d09a6f845ba1125f1447ca8ff110dfb6149a0862f21845cff9c7b126eeb`.
The canonical-target contract was approved at revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`,
and integrated at revision `dfe7ba017ebef525310db8b8ab4ead58fae2d147`,
design SHA-256
`9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b`.
Consumers receive only `CanonicalTarget` from `canonical_target(&Path) ->
Result<CanonicalTarget, StagingError>`, inspect it through `as_path()`, and add
no unchecked constructor or second normalizer.

This design publication changes no runtime or shared canonical artifact. The
paired implementation owns those changes after design review.
