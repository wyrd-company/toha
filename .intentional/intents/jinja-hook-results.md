---
toha: minor
---

Use an opted-in hook result in later hooks and after-apply messages through its
exit code and captured stdout or stderr. Hooks can parse stdout as JSON and
explicitly tolerate nonzero exits, so later hooks can respond to earlier results
without exposing captured output in generated files.
