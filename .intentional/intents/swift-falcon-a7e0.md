---
toha: minor
---

Refuse a git URL that holds a password, such as `https://user:password@host/repo.git`, as git does with `transfer.credentialsInUrl=die`. A command-line argument, template address, configured host, answers document `template` or `git` question answer that holds one is refused before Toha fetches, records or shows it, and the refusal names no part of the URL. A Git credential helper, or a library caller's credential resolver, supplies the password instead. A user name alone, as in `ssh://git@host/repo.git`, is still accepted. This is a breaking change for anyone who put a password in a template address or a `git` answer.
