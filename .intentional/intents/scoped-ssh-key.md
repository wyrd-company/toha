---
toha: minor
toha-docs: patch
---

Library callers can choose the SSH key of one operation's Git connections: `toha::source::with_ssh_key` makes every SSH connection the operation opens on the calling thread (template fetches, commit lookups and the `git` question's remote probe) offer only the named public key from an SSH agent, the one at `SSH_AUTH_SOCK` or a socket that `SshKey::with_agent` names. Operations on other threads, and code after the call, keep offering every agent key. A key the agent does not hold fails the connection with an error that names the key's type and fingerprint, and no other key is tried. Using Toha from Rust documents the scope and its limits.
