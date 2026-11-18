---
toha: major
---

Upgrade the public Rust API to jiff 1 and gix 0.90. Callers must replace jiff::Zoned with jiff::ZonedDateTime and use gix 0.90 types. Refresh dependencies and release tools while retaining MiniJinja 2 rendering semantics; protect credentials in escaped Git error diagnostics.
