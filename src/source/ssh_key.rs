// ---
// relationships:
//   implements: architecture
// ---
//! The SSH key a library caller chooses for the SSH Git remotes of one
//! operation, picked from an SSH agent.
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    io::Write,
    path::{Path, PathBuf},
};

/// A public key that an SSH agent holds, for the SSH remotes of one
/// operation, and the agent that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKey {
    algorithm: String,
    encoded: String,
    blob: Vec<u8>,
    agent: Option<PathBuf>,
}

/// The text given to [`SshKey::parse`] is not an SSH public key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not an SSH public key in authorized-keys form: {0}")]
pub struct SshKeyError(&'static str);

impl SshKey {
    /// Parse a public key in authorized-keys form, as `ssh-add -L` prints it:
    /// the key type, then the base64 key, then an optional comment, such as
    /// `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA…`. Key options before the type,
    /// as an `authorized_keys` line can hold, are not accepted.
    ///
    /// The key is picked from the agent at `SSH_AUTH_SOCK`, read when the
    /// operation connects, unless [`with_agent`](Self::with_agent) names one.
    pub fn parse(public_key: &str) -> Result<Self, SshKeyError> {
        let mut words = public_key.split_whitespace();
        let (Some(algorithm), Some(encoded)) = (words.next(), words.next()) else {
            return Err(SshKeyError("it needs a key type and a base64 key"));
        };
        let blob = STANDARD
            .decode(encoded)
            .map_err(|_| SshKeyError("the key is not base64"))?;
        let named = blob
            .get(..4)
            .map(|len| u32::from_be_bytes(len.try_into().unwrap()) as usize)
            .and_then(|len| blob.get(4..4 + len));
        if named != Some(algorithm.as_bytes()) {
            return Err(SshKeyError("the key is not of the type it names"));
        }
        Ok(Self {
            algorithm: algorithm.to_owned(),
            encoded: encoded.to_owned(),
            blob,
            agent: None,
        })
    }

    /// Pick the key from the agent listening at `socket` instead of the one
    /// at `SSH_AUTH_SOCK`.
    pub fn with_agent(mut self, socket: impl Into<PathBuf>) -> Self {
        self.agent = Some(socket.into());
        self
    }

    /// The key type, such as `ssh-ed25519`.
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    /// The SHA-256 fingerprint, as `ssh-keygen -l` prints it:
    /// `SHA256:` and the unpadded base64 digest.
    pub fn fingerprint(&self) -> String {
        format!(
            "SHA256:{}",
            STANDARD_NO_PAD.encode(Sha256::digest(&self.blob))
        )
    }
}

impl std::str::FromStr for SshKey {
    type Err = SshKeyError;
    fn from_str(public_key: &str) -> Result<Self, SshKeyError> {
        Self::parse(public_key)
    }
}

/// The key type and fingerprint, such as `ssh-ed25519 SHA256:…`.
impl std::fmt::Display for SshKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.algorithm, self.fingerprint())
    }
}

thread_local! {
    static KEY: RefCell<Option<SshKey>> = const { RefCell::new(None) };
}

/// Run `operation` with every SSH connection it opens offering only `key`:
/// template fetches, commit lookups and the `git` question's remote probe.
///
/// The key applies on the calling thread until `operation` returns or
/// unwinds. Operations on other threads, and code after this call, keep the
/// behaviour without a key. An inner call replaces the key for its own
/// operation. HTTPS and local remotes never use it.
///
/// Before each SSH connection, Toha asks the agent for its keys. When the
/// agent does not hold `key`, or cannot be reached, the connection fails with
/// an error that names the key's type and fingerprint, and no other key is
/// tried. Otherwise the connection runs the configured `ssh` program
/// (`core.sshCommand`, `GIT_SSH_COMMAND` or `GIT_SSH`, else `ssh` on `PATH`)
/// with the OpenSSH options `IdentitiesOnly=yes`, `IdentityFile` naming a
/// file that holds the public key, and `IdentityAgent` naming the agent.
/// Whether a remote is SSH is decided after Git's `url.<base>.insteadOf`
/// rewrites.
///
/// Before it connects, Toha runs that command with `ssh -G`. When the
/// configuration it prints for the remote names another `IdentityFile`, or
/// sets `IdentitiesOnly` or `IdentityAgent` otherwise, the connection fails
/// with an error that names the setting and the key. The program must
/// therefore accept OpenSSH options, including `-G`.
pub fn with_ssh_key<T>(key: SshKey, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<SshKey>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            KEY.with(|current| *current.borrow_mut() = previous);
        }
    }
    let _restore = Restore(KEY.with(|current| current.replace(Some(key))));
    operation()
}

/// The calling thread's key, for a thread that works for its operation.
pub(crate) fn current_ssh_key() -> Option<SshKey> {
    KEY.with(|current| current.borrow().clone())
}

/// Whether the calling thread's operation has a key.
pub(crate) fn scoped() -> bool {
    KEY.with(|current| current.borrow().is_some())
}

/// The scoped key that `repo`'s connections to `url` must offer, once its
/// agent has shown that it holds it, or `None` when no key is in scope or
/// `url` is not an SSH remote once gix has applied the `url.<base>.insteadOf`
/// rewrites `repo` configures.
pub(crate) fn for_remote(repo: &gix::Repository, url: &str) -> Result<Option<Held>, String> {
    let Some(key) = current_ssh_key() else {
        return Ok(None);
    };
    let remote = repo.remote_at(url).map_err(|e| format!("{e:#}"))?;
    let Some(url) = remote
        .url(gix::remote::Direction::Fetch)
        .filter(|url| url.scheme == gix::url::Scheme::Ssh)
        .cloned()
    else {
        return Ok(None);
    };
    let agent = match &key.agent {
        Some(agent) => agent.clone(),
        None => std::env::var_os("SSH_AUTH_SOCK")
            .filter(|socket| !socket.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| format!("no SSH agent for the key {key}: SSH_AUTH_SOCK is not set"))?,
    };
    match agent::holds(&agent, &key.blob) {
        Ok(true) => Ok(Some(Held { key, agent, url })),
        Ok(false) => Err(format!(
            "the SSH agent at {} does not offer the key {key}",
            agent.display()
        )),
        Err(error) => Err(format!(
            "could not ask the SSH agent at {} for the key {key}: {error}",
            agent.display()
        )),
    }
}

/// A scoped key that its agent holds, and the SSH remote it is for.
pub(crate) struct Held {
    key: SshKey,
    agent: PathBuf,
    url: gix::Url,
}

/// The in-memory configuration that makes a repository's SSH connections
/// offer only one key, and the public key file it names, which must outlive
/// those connections.
pub(crate) struct Offer {
    pub(crate) overrides: Vec<String>,
    command: String,
    file: String,
    agent: String,
    _file: tempfile::NamedTempFile,
}

impl Held {
    /// The configuration for `repo`, keeping the SSH command it configures,
    /// once `ssh -G` has shown that the remote's SSH configuration adds no
    /// other identity and keeps the options.
    pub(crate) fn offer(&self, repo: &gix::Repository) -> Result<Offer, String> {
        let offer = self.prepare(repo)?;
        self.confirm(&offer)?;
        Ok(offer)
    }

    fn prepare(&self, repo: &gix::Repository) -> Result<Offer, String> {
        let configured = repo.ssh_connect_options().map_err(|e| format!("{e:#}"))?;
        let mut file = tempfile::Builder::new()
            .prefix("toha-ssh-key")
            .suffix(".pub")
            .tempfile()
            .map_err(|e| format!("could not write the SSH key {}: {e}", self.key))?;
        writeln!(file, "{} {}", self.key.algorithm, self.key.encoded)
            .and_then(|()| file.flush())
            .map_err(|e| format!("could not write the SSH key {}: {e}", self.key))?;
        let program = match configured.command.as_ref().filter(|c| !c.is_empty()) {
            Some(command) => {
                let command = command
                    .to_str()
                    .ok_or("the configured SSH command is not UTF-8")?;
                // `GIT_SSH` names a program, which runs without a shell.
                if configured.disallow_shell {
                    quoted(command)
                } else {
                    command.to_owned()
                }
            }
            None => "ssh".to_owned(),
        };
        let key_file = option_path(file.path())?;
        let agent = option_path(&self.agent)?;
        let command = format!(
            "{program} -o IdentitiesOnly=yes -o {} -o {}",
            quoted(&format!("IdentityFile={key_file}")),
            quoted(&format!("IdentityAgent={agent}")),
        );
        Ok(Offer {
            overrides: vec![
                format!("core.sshCommand={command}"),
                "ssh.variant=ssh".to_owned(),
            ],
            command,
            file: key_file,
            agent,
            _file: file,
        })
    }

    /// Fail unless the configuration `ssh -G` prints for the remote, through
    /// the offer's command, names only the offer's identity and agent with
    /// `IdentitiesOnly`. OpenSSH adds every `IdentityFile` it is given and
    /// tries the agent's keys among them in the agent's order, and takes the
    /// first value given for the other two options.
    fn confirm(&self, offer: &Offer) -> Result<(), String> {
        let host = self.url.host().unwrap_or_default();
        let mut args = vec!["-G".to_owned()];
        if let Some(port) = self.url.port {
            args.extend(["-p".to_owned(), port.to_string()]);
        }
        args.push(match self.url.user() {
            Some(user) => format!("{user}@{host}"),
            None => host.to_owned(),
        });
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{} \"$@\"", offer.command))
            .arg("ssh")
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("could not run ssh -G for the key {}: {e}", self.key))?;
        if !output.status.success() {
            return Err(format!(
                "ssh -G for the key {} failed: {}",
                self.key,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let printed = String::from_utf8_lossy(&output.stdout);
        conflict(&printed, offer).map_or(Ok(()), |problem| {
            Err(format!(
                "the SSH configuration for {host} {problem}, so the connection would not \
                 offer only the key {}",
                self.key
            ))
        })
    }
}

/// What in `printed`, the output of `ssh -G`, keeps a connection from
/// offering only `offer`'s identity from `offer`'s agent.
fn conflict(printed: &str, offer: &Offer) -> Option<String> {
    let expected = [
        "identitiesonly yes".to_owned(),
        format!("identityagent {}", offer.agent),
        format!("identityfile {}", offer.file),
    ];
    let mut found = 0;
    for line in printed.lines() {
        let name = line.split_once(' ').map_or(line, |(name, _)| name);
        if ["identitiesonly", "identityagent", "identityfile"].contains(&name) {
            if !expected.iter().any(|expected| expected == line) {
                return Some(format!("sets `{line}`"));
            }
            found += 1;
        }
    }
    (found < expected.len()).then(|| "does not take Toha's OpenSSH options".to_owned())
}

/// `path` as an OpenSSH option value, where `%` starts a token.
fn option_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|path| path.replace('%', "%%"))
        .ok_or_else(|| format!("the path {} is not UTF-8", path.display()))
}

/// `word` as one word of a POSIX shell command.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

mod agent {
    //! Listing the keys of an SSH agent, as `ssh-add -L` does.
    use std::io::{Error, ErrorKind};
    use std::path::Path;

    #[cfg(unix)]
    pub(super) fn holds(socket: &Path, blob: &[u8]) -> std::io::Result<bool> {
        use std::io::{Read, Write};
        const REQUEST_IDENTITIES: u8 = 11;
        // OpenSSH's agent refuses a longer message.
        const MAX_MESSAGE: usize = 256 * 1024;
        let timeout = Some(std::time::Duration::from_secs(10));
        let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
        stream.write_all(&[0, 0, 0, 1, REQUEST_IDENTITIES])?;
        let mut length = [0; 4];
        stream.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_MESSAGE {
            return Err(Error::new(ErrorKind::InvalidData, "the answer is too long"));
        }
        let mut answer = vec![0; length];
        stream.read_exact(&mut answer)?;
        lists(&answer, blob)
    }

    #[cfg(not(unix))]
    pub(super) fn holds(_socket: &Path, _blob: &[u8]) -> std::io::Result<bool> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "a scoped SSH key needs a Unix agent socket",
        ))
    }

    /// Whether `answer`, an agent's answer to a key listing without its
    /// length, lists `blob`.
    pub(super) fn lists(answer: &[u8], blob: &[u8]) -> std::io::Result<bool> {
        const IDENTITIES_ANSWER: u8 = 12;
        let Some((&kind, rest)) = answer.split_first() else {
            return Err(malformed());
        };
        if kind != IDENTITIES_ANSWER {
            return Err(Error::other("the agent refused to list its keys"));
        }
        let mut reader = Reader(rest);
        let count = reader.u32()?;
        for _ in 0..count {
            let key = reader.string()?;
            let _comment = reader.string()?;
            if key == blob {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn malformed() -> Error {
        Error::new(ErrorKind::InvalidData, "the answer is malformed")
    }

    struct Reader<'a>(&'a [u8]);

    impl<'a> Reader<'a> {
        fn take(&mut self, len: usize) -> std::io::Result<&'a [u8]> {
            let (taken, rest) = self.0.split_at_checked(len).ok_or_else(malformed)?;
            self.0 = rest;
            Ok(taken)
        }

        fn u32(&mut self) -> std::io::Result<u32> {
            Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
        }

        fn string(&mut self) -> std::io::Result<&'a [u8]> {
            let len = self.u32()?;
            self.take(len as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generated key, and its fingerprint as `ssh-keygen -l` prints it.
    const SAMPLE: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEI0dunGv3q95XjI4gMbLOj6Jyq9dSS8WXB1W2Df4u6U";
    const FINGERPRINT: &str = "SHA256:nQpreSffQ9djmqcCYLK2VAzke74gO1kFga1ppEfjsaI";

    fn sample() -> SshKey {
        SshKey::parse(SAMPLE).unwrap()
    }

    #[test]
    fn a_public_key_parses_with_or_without_a_comment() {
        let key = sample();
        assert_eq!(key.algorithm(), "ssh-ed25519");
        assert_eq!(key.fingerprint(), FINGERPRINT);
        assert_eq!(key.to_string(), format!("ssh-ed25519 {FINGERPRINT}"));
        let commented: SshKey = format!("  {SAMPLE} sample comment\n").parse().unwrap();
        assert_eq!(commented, key);
    }

    #[test]
    fn other_text_is_not_a_public_key() {
        let [_, encoded] = SAMPLE.split_once(' ').map(|(a, b)| [a, b]).unwrap();
        for text in [
            "",
            "ssh-ed25519",
            "ssh-ed25519 not*base64",
            &format!("ssh-rsa {encoded}"),
            &format!("restrict {SAMPLE}"),
            "ssh-ed25519 AAAA",
        ] {
            assert!(SshKey::parse(text).is_err(), "{text:?} parsed");
        }
    }

    fn is_current(key: &SshKey) -> bool {
        current_ssh_key().as_ref() == Some(key)
    }

    #[test]
    fn a_key_applies_only_inside_its_operation() {
        let outer = sample();
        let inner = sample().with_agent("/sample/inner.sock");
        assert!(current_ssh_key().is_none());
        with_ssh_key(outer.clone(), || {
            assert!(is_current(&outer));
            with_ssh_key(inner.clone(), || assert!(is_current(&inner)));
            assert!(is_current(&outer));
            std::thread::spawn(|| assert!(current_ssh_key().is_none()))
                .join()
                .unwrap();
        });
        assert!(current_ssh_key().is_none());
    }

    #[test]
    fn an_unwinding_operation_restores_the_previous_key() {
        let unwound = std::panic::catch_unwind(|| {
            with_ssh_key(sample(), || panic!("sample operation fails"))
        });
        assert!(unwound.is_err());
        assert!(current_ssh_key().is_none());
    }

    /// A repository whose configuration rewrites `https://rewritten.invalid/`
    /// to an SSH remote.
    fn repository() -> (tempfile::TempDir, gix::Repository) {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = gix::init_bare(dir.path()).unwrap();
        repo.config_snapshot_mut()
            .append_config(
                ["url.ssh://git@host.invalid/.insteadOf=https://rewritten.invalid/"],
                gix::config::Source::Api,
            )
            .unwrap();
        (dir, repo)
    }

    #[test]
    fn only_ssh_remotes_use_the_key() {
        let (_dir, repo) = repository();
        let for_remote = |url| for_remote(&repo, url);
        // The agent does not exist, so a remote that used the key would fail.
        let key = sample().with_agent("/nonexistent/agent.sock");
        with_ssh_key(key, || {
            for url in [
                "https://host.invalid/r.git",
                "file:///sample/r.git",
                "/sample/r.git",
            ] {
                assert!(for_remote(url).unwrap().is_none(), "{url}");
            }
            for url in [
                "ssh://git@host.invalid/r.git",
                "git@host.invalid:o/r.git",
                "https://rewritten.invalid/r.git",
            ] {
                let error = for_remote(url).err().unwrap();
                assert!(error.contains(FINGERPRINT), "{error}");
            }
        });
        assert!(
            for_remote("ssh://git@host.invalid/r.git")
                .unwrap()
                .is_none()
        );
    }

    /// An agent's answer to a key listing, listing `keys`.
    fn answer(keys: &[&[u8]]) -> Vec<u8> {
        let mut answer = vec![12];
        answer.extend((keys.len() as u32).to_be_bytes());
        for key in keys {
            for field in [*key, b"comment".as_slice()] {
                answer.extend((field.len() as u32).to_be_bytes());
                answer.extend(field);
            }
        }
        answer
    }

    #[test]
    fn a_listing_holds_the_key_only_when_it_lists_its_blob() {
        let blob = sample().blob;
        assert!(agent::lists(&answer(&[b"other", &blob]), &blob).unwrap());
        assert!(!agent::lists(&answer(&[b"other"]), &blob).unwrap());
        assert!(!agent::lists(&answer(&[]), &blob).unwrap());
        // The agent's failure answer, and a listing cut short.
        assert!(agent::lists(&[5], &blob).is_err());
        let listed = answer(&[&blob]);
        assert!(agent::lists(&listed[..listed.len() - 1], &blob).is_err());
    }

    /// An agent at a socket in `dir` that answers one key listing with
    /// `answer`, after a length of `length`, and the request it received.
    #[cfg(unix)]
    fn fake_agent(
        dir: &Path,
        length: usize,
        answer: Vec<u8>,
    ) -> (PathBuf, std::thread::JoinHandle<Vec<u8>>) {
        use std::io::Read;
        let socket = dir.join("agent.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            stream.write_all(&(length as u32).to_be_bytes()).unwrap();
            // A client that refuses the length has hung up.
            let _ = stream.write_all(&answer);
            request.to_vec()
        });
        (socket, served)
    }

    /// An agent at a socket in `dir` that lists `keys`.
    #[cfg(unix)]
    fn listing_agent(dir: &Path, keys: &[&[u8]]) -> (PathBuf, std::thread::JoinHandle<Vec<u8>>) {
        let answer = answer(keys);
        fake_agent(dir, answer.len(), answer)
    }

    #[cfg(unix)]
    #[test]
    fn an_answer_longer_than_an_agent_sends_is_refused() {
        const MAX: usize = 256 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let (socket, served) = fake_agent(dir.path(), MAX + 1, Vec::new());
        let error = agent::holds(&socket, &sample().blob).unwrap_err();
        served.join().unwrap();
        assert_eq!(error.to_string(), "the answer is too long");
        // The longest answer an agent sends is read: one key and a comment
        // that fills it.
        let blob = sample().blob;
        let mut longest = vec![12, 0, 0, 0, 1];
        longest.extend((blob.len() as u32).to_be_bytes());
        longest.extend(&blob);
        let comment = MAX - longest.len() - 4;
        longest.extend((comment as u32).to_be_bytes());
        longest.resize(MAX, b'c');
        let dir = tempfile::tempdir().unwrap();
        let (socket, served) = fake_agent(dir.path(), MAX, longest);
        assert!(agent::holds(&socket, &blob).unwrap());
        served.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_that_holds_the_key_lets_the_connection_offer_it() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, served) = listing_agent(dir.path(), &[b"other", &sample().blob]);
        let (_dir, repo) = repository();
        let held = with_ssh_key(sample().with_agent(&socket), || {
            for_remote(&repo, "https://rewritten.invalid/r.git")
                .unwrap()
                .unwrap()
        });
        assert_eq!(held.agent, socket);
        assert_eq!(held.url.to_bstring(), "ssh://git@host.invalid/r.git");
        assert_eq!(served.join().unwrap(), [0, 0, 0, 1, 11]);
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_without_the_key_fails_the_connection_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, served) = listing_agent(dir.path(), &[b"other"]);
        let (_dir, repo) = repository();
        let error = with_ssh_key(sample().with_agent(&socket), || {
            for_remote(&repo, "ssh://git@host.invalid/r.git")
                .err()
                .unwrap()
        });
        served.join().unwrap();
        assert_eq!(
            error,
            format!(
                "the SSH agent at {} does not offer the key ssh-ed25519 {FINGERPRINT}",
                socket.display()
            )
        );
    }

    #[test]
    fn an_unreachable_agent_fails_the_connection_naming_the_key() {
        let (_dir, repo) = repository();
        let error = with_ssh_key(sample().with_agent("/nonexistent/agent.sock"), || {
            for_remote(&repo, "ssh://git@host.invalid/r.git")
                .err()
                .unwrap()
        });
        let expected = format!(
            "could not ask the SSH agent at /nonexistent/agent.sock for the key ssh-ed25519 {FINGERPRINT}: "
        );
        assert!(error.starts_with(&expected), "{error}");
    }

    /// The SSH command and variant a repository with `offer` applied
    /// connects with.
    fn applied(repo: &mut gix::Repository, offer: &Offer) -> (String, Option<String>) {
        repo.config_snapshot_mut()
            .append_config(&offer.overrides, gix::config::Source::Api)
            .unwrap();
        let options = repo.ssh_connect_options().unwrap();
        let command = options.command.unwrap().into_string().unwrap();
        (command, options.kind.map(|kind| format!("{kind:?}")))
    }

    fn held(agent: &str) -> Held {
        Held {
            key: sample(),
            agent: agent.into(),
            url: gix::url::parse(gix::bstr::BStr::new("ssh://git@host.invalid/r.git")).unwrap(),
        }
    }

    #[test]
    fn an_offer_names_only_the_key_file_and_its_agent() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = gix::init_bare(dir.path()).unwrap();
        repo.config_snapshot_mut()
            .set_raw_value("core.sshCommand", "ssh -F '/sample/ssh config'")
            .unwrap();
        let held = held("/sample/it's 100%/agent.sock");
        let offer = held.prepare(&repo).unwrap();
        let file = offer._file.path().to_str().unwrap().to_owned();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("{SAMPLE}\n")
        );
        let (command, kind) = applied(&mut repo, &offer);
        assert_eq!(
            command,
            format!(
                "ssh -F '/sample/ssh config' -o IdentitiesOnly=yes -o 'IdentityFile={file}' \
                 -o 'IdentityAgent=/sample/it'\\''s 100%%/agent.sock'"
            )
        );
        assert_eq!(kind.as_deref(), Some("Ssh"));
    }

    #[test]
    fn an_offer_without_a_configured_command_runs_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = gix::init_bare(dir.path()).unwrap();
        // Clear any `core.sshCommand` the environment configures.
        repo.config_snapshot_mut()
            .set_raw_value("core.sshCommand", "")
            .unwrap();
        let offer = held("/sample/agent.sock").prepare(&repo).unwrap();
        let (command, _) = applied(&mut repo, &offer);
        assert!(
            command.starts_with("ssh -o IdentitiesOnly=yes "),
            "{command}"
        );
    }

    #[test]
    fn only_the_offer_s_identity_agent_and_identities_only_pass() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        let offer = held("/sample/agent.sock").prepare(&repo).unwrap();
        let ours = format!(
            "user git\nidentitiesonly yes\nidentityagent /sample/agent.sock\n\
             identityfile {}\nport 22\n",
            offer.file
        );
        assert_eq!(conflict(&ours, &offer), None);
        for (extra, problem) in [
            (
                "identityfile ~/.ssh/other",
                "sets `identityfile ~/.ssh/other`",
            ),
            ("identitiesonly no", "sets `identitiesonly no`"),
            (
                "identityagent /other.sock",
                "sets `identityagent /other.sock`",
            ),
        ] {
            let printed = format!("{extra}\n{ours}");
            assert_eq!(conflict(&printed, &offer).as_deref(), Some(problem));
        }
        assert_eq!(
            conflict("user git\nidentitiesonly yes\n", &offer).as_deref(),
            Some("does not take Toha's OpenSSH options")
        );
    }
}
