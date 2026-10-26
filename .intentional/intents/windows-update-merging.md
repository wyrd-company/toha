---
toha: patch
---

Merge template updates correctly when automatic line-ending conversion is in effect and the repository already holds CRLF content. Preserve operator content edits and Git attributes. Refuse templates that overwrite the applied record on Windows.

Keep read-only comparison and in-memory merge-input normalization independent of safecrlf check-in checks while retaining the repository policy for candidate admission. Update preparation can write internal Git objects.
