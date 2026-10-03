---
toha: minor
---

Record rendered files and their answers as Git snapshots after applies into
clean, committed repositories. List and remove snapshots with `snapshots list`
and `snapshots clean`, and use `init` to configure a remote fetch refspec for
sharing snapshots across clones. The Rust crate exposes snapshot capture,
lookup, and merge APIs.
