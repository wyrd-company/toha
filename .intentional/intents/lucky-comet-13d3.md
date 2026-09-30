---
toha: minor
---

Add in-project generators: an additive `--like [SELECTOR]` flag on `apply` and `stage` that seeds a new application's answer defaults from a snapshot of a prior application of the same source (the generate axis, a sibling of `--from`). Selection is repository-wide and source-filtered; precedence is route answer > snapshot > configured/template default; the staged record pins the chosen snapshot and re-reads it on resume; `applied`/`questions` documents gain an optional `seed` member. No `--like` leaves every route byte-identical.
