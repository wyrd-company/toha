---
toha: patch
---

Validate early answers according to whether their questions remain active, are
skipped, or are not yet resolved. Omit errors for questions proven skipped even
when another answer fails, and keep rejected submissions atomic without leaking
messages, warnings, or hook effects.
