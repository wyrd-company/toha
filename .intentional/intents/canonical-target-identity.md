---
toha: patch
---

Remove redundant trailing separators from existing-directory targets so staged
state, protocol context, and file planning use the same identity. Preserve
resume and abort of 0.1.0 staged records. Rust target-consuming APIs use
`CanonicalTarget` from `canonical_target` in place of raw paths.
