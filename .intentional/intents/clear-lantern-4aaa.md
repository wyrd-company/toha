---
toha: patch
---

Report a failed operation on a Git remote in Toha's own words, as git does, such as `authentication failed for '<url>'`, instead of the Git transport's text. Show, record and return remote URLs without user information, read from the URL's structure. Keep credentials from a template address out of the template clones Toha keeps, their configuration and their reflogs. Remove the credentials from a `git` question's answer and default as Toha takes them in, and from what its `format` makes of them, so they reach no staged state, document, prompt, report or remote; supply them with a git credential helper. An SSH address keeps its user. Keep curl's verbose output off for every HTTP connection, as it writes the request headers that carry credentials, and fetch a remote branch named `HEAD` only when the address selects it, so that no clone retries on a connection Toha does not prepare.
