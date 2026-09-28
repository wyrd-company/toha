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
- Version: 5
- Approved design revision:
  `6531546a113f672a10d47c19c54ed700b75c9165`
- Epic base and merge-base:
  `a62061fdce74b6b1a743c70565dd9fbeec2413bb`
- Portable source: `brief.source.html`
- Built deck: `brief.deck.html`
- Design SHA-256:
  `50511d19f2a533b4a88e3e7a5b068159d4b5263f3516163a33cfa96d8a3c5fe4`
- Verification SHA-256:
  `6721d4953e7bee77ac59858e17a99ca37d2290a27ff8a6c2e49c1d1af689626b`
- Brief source SHA-256:
  `0b25c446cd96ca5db49432219686735e60c46cba9d9190b939826c1de61c87f4`
- Built and published raw SHA-256:
  `d8e842d6c253a048b78dc7984d75515a51accd86ac7cba5a24407b9adc73860b`
- Built and published raw bytes: 318477
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

The exact epic base contains the configured-presets implementation, the
hook-review contract, the Jinja-include design, and the canonical-target
producer. Presets remain answer defaults and are not Jinja context.
`ConfigEntry` remains the sole origin source. `configured_defaults` carries each
winning mapping and optional preset origin in private `ResolvedDefault` entries.
Configured start and replay consume that `Resolution` into
`DefaultBankEntry::Configured` with the invocation context; only the separate
ordinary `Seed.defaults` route stays flat. Context adds no second origin map or
resolver.

The include binding is revision
`b87a5482b97e81524272bee1e526698c862754db`, final-design SHA-256
`3c996d09a6f845ba1125f1447ca8ff110dfb6149a0862f21845cff9c7b126eeb`.
The canonical-target contract was approved at revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`,
and integrated at revision `dfe7ba017ebef525310db8b8ab4ead58fae2d147`,
design SHA-256
`9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
Consumers receive only `CanonicalTarget` from `canonical_target(&Path) ->
Result<CanonicalTarget, StagingError>`, inspect it through `as_path()`, and add
no unchecked constructor or second normalizer.

This design publication changes no runtime or shared canonical artifact. The
paired implementation owns those changes after design review.
