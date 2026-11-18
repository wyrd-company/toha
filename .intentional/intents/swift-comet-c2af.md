---
toha: major
---

Let a library caller stop a template fetch, a remote probe, an apply or an update with a stop signal (`toha::stop`). A stopped call returns the new `Stopped` variant of its error and writes no further file, and a fetch held by a remote that sends nothing returns at once. Limit: gix cannot interrupt a waiting transfer, so the transfer a stop abandons keeps its Toha thread, gix's curl thread and the connection over HTTP(S), or its Toha thread and the `ssh` or `git-upload-pack` process over SSH and `file://`, until the remote answers or closes, or Git's `http.lowSpeedLimit` ends an HTTP(S) transfer; it then writes nothing more and removes its folder (wyrd-company/toha#78). `SourceError`, `ResolveError`, `CatalogError`, `UpdateError`, `ApplyError`, `MergeError`, `GitTargetError` and `RemoteState` gain a `Stopped` variant.
