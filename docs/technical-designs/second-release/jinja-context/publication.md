---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - error-attribution
---

# Jinja context Phase C publication

- Postplan URL: <https://t30jlda1nnbd.postplan.dev>
- Draft ID: `t30jlda1nnbd`
- Version: 2
- Portable source: `brief.source.html`
- Built deck: `brief.deck.html`
- Source SHA-256:
  `d41eddf6521f20ce5943142a676fbd13c2e44a3e8c0e3387323c0fd258e89a8d`
- Built and published raw SHA-256:
  `04aa0bc1cdb7f764bf9f17a874883d3adeaf9550e9860327c7130448c60a33c4`
- Published wrapper `Reveal.initialize` count: 1
- Published horizontal positions: 14
- Published slide count: 27
- Epic base and merge-base:
  `4738042a766dc7b2d1d3c76a9c83c41c03e803d2`

The local built deck and published raw deck are byte-identical. All 27 slides,
including every horizontal and vertical position, were rendered and inspected
at 1280×720. Programmatic bounds checks found no viewport or body overflow.
Dense seam, before/after, behavior, and decision slides were also inspected at
original resolution. Bordered code panels remain inside the viewport and no
slide requires scrolling.

The repository `task ci` gate passes. The epic base contains the configured
defaults design. The independent Jinja-include design remains outside this base
and is not a prerequisite of this context design.

The canonical task execution log names the exact checkpoint commit. Bob's
approval must name or unambiguously answer that revision and its access
decisions; publication alone is not approval.
