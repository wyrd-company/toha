---
toha: major
---

A library caller's stop signal (`toha::stop`) now also ends a template load that is fetching an HTTPS document schema: the stop aborts the fetch and closes its connection within a fraction of a second (Toha checks the signal at least every 100 ms), even while the endpoint sends nothing. `LoadError` gains a `stopped` field that marks such a load. Update preparation reports it as `UpdateError::Stopped`, and catalog operations report it as `CatalogError::Stopped`. Without a stop signal, a schema fetch behaves as before.
