# Changelog

## 0.1.3

### Fixes

- Library callers can choose the SSH key of one operation's Git connections: `toha::source::with_ssh_key` makes every SSH connection the operation opens on the calling thread (template fetches, commit lookups and the `git` question's remote probe) offer only the named public key from an SSH agent, the one at `SSH_AUTH_SOCK` or a socket that `SshKey::with_agent` names. Operations on other threads, and code after the call, keep offering every agent key. A key the agent does not hold fails the connection with an error that names the key's type and fingerprint, and no other key is tried. So does an SSH configuration that names another identity, which Toha checks with `ssh -G` before it connects. Using Toha from Rust documents the scope and its limits.

## 0.1.2

### Fixes

- Explain that Toha refuses a git address, configured host or `git` answer that holds a password, and that a Git credential helper supplies it instead.

## 0.1.1

### Fixes

- Give the Rust library guide its own Library navigation section.
- Clarify explicit HEAD-branch selection inside and outside a credential resolver scope and document the thread-scoped resolver input.
