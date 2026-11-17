---
toha: major
---

A library caller's stop signal (`toha::stop`) now also ends a template load that is fetching an HTTPS document schema: the stop aborts the fetch and closes its connection within a fraction of a second (Toha checks the signal at least every 100 ms), even while the endpoint sends nothing. `LoadError` gains a `stopped` field that marks such a load. Update preparation reports it as `UpdateError::Stopped`, and catalog operations report it as `CatalogError::Stopped`. Without a stop signal, a schema fetch behaves as before. Limit: a name lookup can't be interrupted, so a stop that arrives while the schema host's lookup is still pending detaches curl's resolver threads for it; they hold no connection and end when the system resolver returns (wyrd-company/toha#81). With a libcurl that supports neither quick-exit option, the stop waits for the lookup.
