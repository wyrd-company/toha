---
toha: major
---

Let a library caller stop a template fetch, a remote probe, an apply or an update with a stop signal (`toha::stop`). A stopped call returns the new `Stopped` variant of its error and writes no further file, and a fetch held by a remote that sends nothing returns at once. `SourceError`, `ResolveError`, `CatalogError`, `UpdateError`, `ApplyError`, `MergeError`, `GitTargetError` and `RemoteState` gain a `Stopped` variant.
