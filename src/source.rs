//! Template addresses and immutable Git fetches.
use base64::Engine;
use gix::bstr::ByteSlice;
use indexmap::IndexMap;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
mod credentials;
pub use credentials::{
    Credential, CredentialRequest, CredentialResolver, with_credential_resolver,
};
pub(crate) use credentials::{Supplied, current_resolver};
mod ssh_key;
pub use ssh_key::{SshKey, SshKeyError, with_ssh_key};
pub(crate) use ssh_key::{current_ssh_key, for_remote as ssh_key_for_remote};
pub type Hosts = IndexMap<String, String>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    Git {
        repo: String,
        reference: Option<String>,
        path: Option<String>,
    },
    Folder(PathBuf),
    Name(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Branch,
    Tag,
    Commit,
    DefaultBranch,
}
#[derive(Debug, Clone)]
pub struct Fetched {
    pub commit: String,
    pub reference_kind: RefKind,
}
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("invalid template address: {0}")]
    Address(String),
    #[error("{0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("git fetch failed: {0}")]
    Git(String),
    /// The operation's [stop signal](crate::stop) fired.
    #[error("the fetch was stopped")]
    Stopped,
}
impl From<crate::stop::Stopped> for SourceError {
    fn from(_: crate::stop::Stopped) -> Self {
        Self::Stopped
    }
}
fn split_git(value: &str) -> Result<(&str, Option<String>, Option<String>), SourceError> {
    let (before_path, path) = value
        .split_once('#')
        .map_or((value, None), |(a, b)| (a, Some(b.to_string())));
    if path.as_deref() == Some("") {
        return Err(SourceError::Address("empty template path".into()));
    }
    let start = if before_path.starts_with("git@") {
        before_path.find(':').map_or(0, |p| p + 1)
    } else {
        before_path.find("://").map_or(0, |p| {
            let authority = p + 3;
            before_path[authority..]
                .find('/')
                .map_or(before_path.len(), |slash| authority + slash + 1)
        })
    };
    let (repo, reference) = before_path[start..]
        .rfind('@')
        .map_or((before_path, None), |p| {
            (
                &before_path[..start + p],
                Some(before_path[start + p + 1..].to_string()),
            )
        });
    if repo.is_empty() || reference.as_deref() == Some("") {
        return Err(SourceError::Address("invalid repository or ref".into()));
    }
    Ok((repo, reference, path))
}
pub fn parse(arg: &str, hosts: &Hosts, cwd: &Path, home: &Path) -> Result<Address, SourceError> {
    if arg.contains("://") || arg.starts_with("git@") {
        if has_password(arg) {
            return Err(SourceError::Address(PASSWORD_REFUSED.into()));
        }
        let (repo, reference, path) = split_git(arg)?;
        return Ok(Address::Git {
            repo: repo.into(),
            reference,
            path,
        });
    }
    if let Some((prefix, rest)) = arg.split_once(':')
        && let Some(base) = hosts.get(prefix)
    {
        let (repo, reference, path) = split_git(rest)?;
        if !repo.contains('/') {
            return Err(SourceError::Address(
                "host address requires owner/repository".into(),
            ));
        }
        let repo = format!("{}/{}", base.trim_end_matches('/'), repo);
        if has_password(&repo) {
            return Err(SourceError::Address(format!(
                "the configured host `{prefix}`: {PASSWORD_REFUSED}"
            )));
        }
        return Ok(Address::Git {
            repo,
            reference,
            path,
        });
    }
    let drive = arg.as_bytes().get(0..3).is_some_and(|v| {
        v[0].is_ascii_alphabetic() && v[1] == b':' && matches!(v[2], b'/' | b'\\')
    });
    if arg.starts_with('.') || Path::new(arg).is_absolute() || arg.starts_with('~') || drive {
        let value = if arg == "~" {
            home.to_path_buf()
        } else if let Some(rest) = arg.strip_prefix("~/") {
            home.join(rest)
        } else {
            PathBuf::from(arg)
        };
        let absolute = if value.is_absolute() {
            value
        } else {
            cwd.join(value)
        };
        return Ok(Address::Folder(
            absolute
                .canonicalize()
                .map_err(|e| SourceError::Io(absolute, e))?,
        ));
    }
    Ok(Address::Name(arg.into()))
}
impl Address {
    pub fn formal_name(&self, hosts: &Hosts) -> String {
        match self {
            Self::Name(name) => name.clone(),
            Self::Folder(path) => path.to_string_lossy().into_owned(),
            Self::Git {
                repo,
                reference,
                path,
            } => {
                let safe_repo = without_userinfo(repo);
                let mut name = hosts
                    .iter()
                    .find_map(|(prefix, base)| {
                        let remainder = safe_repo
                            .strip_prefix(base.trim_end_matches('/'))?
                            .strip_prefix('/')?;
                        if remainder.is_empty() || !remainder.contains('/') {
                            return None;
                        }
                        Some(format!("{prefix}:{remainder}"))
                    })
                    .or_else(|| {
                        hosts.iter().find_map(|(prefix, base)| {
                            let host = base.split("://").nth(1)?.trim_end_matches('/');
                            let remainder = repo
                                .strip_prefix("git@")?
                                .strip_prefix(host)?
                                .strip_prefix(':')?;
                            Some(format!("{prefix}:{remainder}"))
                        })
                    })
                    .unwrap_or(safe_repo);
                if name.ends_with(".git") {
                    name.truncate(name.len() - 4);
                }
                if let Some(reference) = reference {
                    name.push('@');
                    name.push_str(reference);
                }
                if let Some(path) = path {
                    name.push('#');
                    name.push_str(path);
                }
                name
            }
        }
    }
}
/// A source suitable for durable registry and cache identifiers.
pub fn without_userinfo(repo: &str) -> String {
    if let Some(scheme) = repo.find("://") {
        let authority_start = scheme + 3;
        let authority_end = repo[authority_start..]
            .find('/')
            .map_or(repo.len(), |slash| authority_start + slash);
        if let Some(at) = repo[authority_start..authority_end].rfind('@') {
            return format!(
                "{}{}",
                &repo[..authority_start],
                &repo[authority_start + at + 1..]
            );
        }
    }
    if let Some(rest) = repo.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("ssh://{host}/{path}");
        }
        return format!("ssh://{rest}");
    }
    repo.to_string()
}
/// Why Toha refuses a git URL that holds a password. It names no part of the
/// URL.
pub const PASSWORD_REFUSED: &str = "a git URL with a password is not accepted; remove the password and let a Git credential helper supply it";
/// Whether the git URL `url` holds a password, as in
/// `https://user:password@host/repo.git`. Toha refuses such a URL, as git does
/// with `transfer.credentialsInUrl=die`, so a password never reaches a clone, a
/// remote, a record or a message. A user name alone, as in
/// `ssh://git@host/repo.git`, is accepted.
pub fn has_password(url: &str) -> bool {
    let parsed = gix::url::parse(gix::bstr::BStr::new(url)).is_ok_and(|u| u.password().is_some());
    // Read the authority as typed as well, so that a URL gix cannot parse,
    // such as one whose password holds `#`, is refused rather than passed on.
    parsed
        || url.split_once("://").is_some_and(|(_, rest)| {
            let authority = rest.split(['/', '?']).next().unwrap_or(rest);
            authority
                .rsplit_once('@')
                .is_some_and(|(userinfo, _)| userinfo.contains(':'))
        })
}
/// The fetch failure for `error`, without the credentials of `repo` or any
/// password `supplied` saw.
fn git_error(error: impl std::fmt::Display, repo: &str, supplied: &Supplied) -> SourceError {
    // gix's alternate Display retains the diagnostic chain before redaction.
    SourceError::Git(supplied.message(redacted(&format!("{error:#}"), repo)))
}
/// `message` with the credentials of `repo` removed wherever it names them.
pub(crate) fn redacted(message: &str, repo: &str) -> String {
    let safe = without_userinfo(repo);
    let mut message = message.to_owned();
    for (form, replacement) in credentials::written_forms(repo)
        .into_iter()
        .zip(credentials::written_forms(&safe))
    {
        message = message.replace(&form, &replacement);
    }
    if let Some(scheme) = repo.find("://") {
        let start = scheme + 3;
        let end = repo[start..]
            .find('/')
            .map_or(repo.len(), |slash| start + slash);
        if let Some(at) = repo[start..end].rfind('@') {
            for form in credentials::written_forms(&repo[start..start + at + 1]) {
                message = message.replace(&form, "");
            }
        }
    }
    message
}
pub fn cache_path(cache: &Path, address: &Address, hosts: &Hosts, commit: &str) -> PathBuf {
    cache
        .join("sources")
        .join(install_key(&address.formal_name(hosts)))
        .join(commit)
}
pub fn install_key(formal: &str) -> String {
    formal
        .split('#')
        .next()
        .unwrap_or(formal)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}
fn reference_kind(repo: &str, reference: Option<&str>) -> Result<RefKind, SourceError> {
    let Some(reference) = reference else {
        return Ok(RefKind::DefaultBranch);
    };
    if let Some(local) = local_git_path(repo) {
        let remote = gix::open(local).map_err(|e| SourceError::Git(e.to_string()))?;
        if remote
            .find_reference(&format!("refs/tags/{reference}"))
            .is_ok()
        {
            return Ok(RefKind::Tag);
        }
        if remote
            .find_reference(&format!("refs/heads/{reference}"))
            .is_ok()
        {
            return Ok(RefKind::Branch);
        }
    }
    if reference.len() >= 4 && reference.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(RefKind::Commit)
    } else {
        Ok(RefKind::Branch)
    }
}
fn local_git_path(repo: &str) -> Option<&str> {
    let local = repo.strip_prefix("file://")?;
    if cfg!(windows) {
        Some(windows_local_git_path(local))
    } else {
        Some(local)
    }
}
fn windows_local_git_path(local: &str) -> &str {
    if local.starts_with('/') && local.as_bytes().get(2) == Some(&b':') {
        &local[1..]
    } else {
        local
    }
}
fn expand_commit(
    repo: &str,
    short: &str,
    dest: &Path,
    interrupt: &AtomicBool,
) -> Result<String, SourceError> {
    if short.len() == 40 {
        return Ok(short.into());
    }
    let local = local_git_path(repo).unwrap_or(repo);
    if repo.starts_with("file://") {
        return gix::open(local)
            .map_err(|e| SourceError::Git(e.to_string()))?
            .rev_parse_single(short)
            .map(|id| id.to_string())
            .map_err(|e| SourceError::Git(e.to_string()));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| SourceError::Address("destination has no parent".into()))?;
    let temporary = tempfile::tempdir_in(parent).map_err(|e| SourceError::Io(parent.into(), e))?;
    let lookup = temporary.path().join("lookup");
    clone_into(
        &Address::Git {
            repo: repo.into(),
            reference: None,
            path: None,
        },
        &lookup,
        interrupt,
    )?;
    gix::open(&lookup)
        .map_err(|e| SourceError::Git(e.to_string()))?
        .rev_parse_single(short)
        .map(|id| id.to_string())
        .map_err(|e| SourceError::Git(e.to_string()))
}
/// The roots Toha hands a bundled rustls transport, written once per connection
/// owner and kept alive for as long as the connection may read them.
pub(crate) type NativeRoots = Option<tempfile::NamedTempFile>;
/// A connection Toha opens to a Git remote.
pub(crate) type Connection<'a, 'b, 'c> = gix::remote::Connection<
    'a,
    'b,
    'c,
    Box<dyn gix::protocol::transport::client::blocking_io::Transport + Send>,
>;
/// Prepare every connection Toha opens: answer HTTPS challenges from the
/// operation's credential resolver, and set the HTTP options.
pub(crate) fn prepare_connection(
    connection: &mut Connection<'_, '_, '_>,
    native_roots: &mut NativeRoots,
    supplied: &Supplied,
) -> gix::Result<()> {
    let resolving = supplied.attach(connection);
    configure_http(connection, native_roots, resolving).map_err(gix::Error::from_boxed)
}
/// Point an HTTPS connection at the platform's trusted roots. System curl
/// backends already use platform trust. Bundled rustls needs a CA bundle when
/// Git has not supplied one. When `quiet`, curl's verbose output is off, as it
/// writes request headers, a resolver's credential among them, to stderr.
fn configure_http(
    connection: &mut Connection<'_, '_, '_>,
    native_roots: &mut NativeRoots,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let rustls = curl::Version::get()
        .ssl_version()
        .is_some_and(|backend| backend.starts_with("rustls"));
    if !rustls && !quiet {
        return Ok(());
    }
    let url = connection.transport_mut().to_url().into_owned();
    let remote = connection.remote();
    let Some(mut options) = remote
        .repo()
        .transport_options(url.as_bstr(), remote.name().map(gix::remote::Name::as_bstr))?
    else {
        return Ok(());
    };
    if let Some(http) =
        options.downcast_mut::<gix::protocol::transport::client::blocking_io::http::Options>()
    {
        if quiet {
            http.verbose = false;
        }
        if rustls && http.ssl_ca_info.is_none() && http.ssl_verify {
            if native_roots.is_none() {
                let loaded = rustls_native_certs::load_native_certs();
                if loaded.certs.is_empty() {
                    // Plain HTTP does not require CA roots. With no bundle,
                    // curl still verifies and rejects an untrusted TLS peer.
                    connection.set_transport_options(options);
                    return Ok(());
                }
                let mut bundle = tempfile::NamedTempFile::new()?;
                for cert in loaded.certs {
                    writeln!(bundle, "-----BEGIN CERTIFICATE-----")?;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(cert.as_ref());
                    for line in encoded.as_bytes().chunks(64) {
                        bundle.write_all(line)?;
                        bundle.write_all(b"\n")?;
                    }
                    writeln!(bundle, "-----END CERTIFICATE-----")?;
                }
                bundle.flush()?;
                *native_roots = Some(bundle);
            }
            http.ssl_ca_info = Some(native_roots.as_ref().unwrap().path().to_owned());
        }
    }
    connection.set_transport_options(options);
    Ok(())
}
/// Clone `address` into `dest`, which must not exist, and check out the
/// selected revision.
///
/// Inside [`with_stop_signal`](crate::stop::with_stop_signal) the clone runs
/// on a thread Toha starts, into a temporary folder beside `dest` that the
/// thread owns, and is moved to `dest` once complete. A stopped fetch returns
/// [`SourceError::Stopped`] at once, even while the remote sends nothing, and
/// leaves `dest` absent. The abandoned clone keeps its thread, and its
/// connection or process, until the remote acts, then removes its folder; see
/// the limit in [`with_stop_signal`](crate::stop::with_stop_signal).
pub fn fetch(address: &Address, dest: &Path) -> Result<Fetched, SourceError> {
    if let Address::Git { repo, .. } = address
        && has_password(repo)
    {
        return Err(SourceError::Address(PASSWORD_REFUSED.into()));
    }
    if !crate::stop::scoped() {
        return clone_into(address, dest, &AtomicBool::new(false));
    }
    if dest.exists() {
        return Err(SourceError::Address(format!(
            "destination exists: {}",
            dest.display()
        )));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| SourceError::Address("destination has no parent".into()))?;
    let (folder, fetched) = fetch_temporary(address, parent, ".tmp")?;
    std::fs::rename(folder.path().join("repo"), dest)
        .map_err(|e| SourceError::Io(dest.to_owned(), e))?;
    Ok(fetched)
}
/// Clone `address` into the `repo` entry of a new temporary folder in
/// `parent`, named with `prefix`. Inside a stop signal's scope the clone runs
/// on a Toha thread that owns the folder until it returns it, so a stopped
/// fetch leaves the folder to that thread, which removes it when it ends.
pub(crate) fn fetch_temporary(
    address: &Address,
    parent: &Path,
    prefix: &'static str,
) -> Result<(tempfile::TempDir, Fetched), SourceError> {
    if let Address::Git { repo, .. } = address
        && has_password(repo)
    {
        return Err(SourceError::Address(PASSWORD_REFUSED.into()));
    }
    let address = address.clone();
    let parent = parent.to_owned();
    crate::stop::run(move |interrupt| {
        let folder = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(&parent)
            .map_err(|e| SourceError::Io(parent.clone(), e))?;
        let fetched = clone_into(&address, &folder.path().join("repo"), interrupt)?;
        Ok((folder, fetched))
    })?
}
/// The options `gix::prepare_clone` opens its repository with.
fn clone_open_options() -> gix::open::Options {
    use gix::sec::trust::DefaultForLevel;
    let mut options = gix::open::Options::default_for_level(gix::sec::Trust::Full);
    options.permissions.config.git_binary = true;
    options
}
/// Clone `address` into `dest` on the calling thread, with `interrupt` as
/// gix's interrupt flag.
fn clone_into(
    address: &Address,
    dest: &Path,
    interrupt: &AtomicBool,
) -> Result<Fetched, SourceError> {
    let Address::Git {
        repo, reference, ..
    } = address
    else {
        return Err(SourceError::Address("a git address is required".into()));
    };
    // An address can come from a registry or record that an earlier Toha
    // wrote with a password, or from a library caller, as well as from `parse`.
    if has_password(repo) {
        return Err(SourceError::Address(PASSWORD_REFUSED.into()));
    }
    let mut kind = reference_kind(repo, reference.as_deref())?;
    if dest.exists() {
        return Err(SourceError::Address(format!(
            "destination exists: {}",
            dest.display()
        )));
    }
    let resolved_commit = if kind == RefKind::Commit {
        Some(expand_commit(
            repo,
            reference.as_deref().unwrap(),
            dest,
            interrupt,
        )?)
    } else {
        None
    };
    let mut native_roots = None;
    let supplied = Supplied::default();
    let attach = supplied.clone();
    let mut clone = gix::prepare_clone(repo.as_str(), dest)
        .map_err(|e| git_error(e, repo, &supplied))?
        .configure_connection(move |connection| {
            prepare_connection(connection, &mut native_roots, &attach)
        });
    // The clone's connections take their remote's URL and SSH command from the
    // configuration of the repository `prepare_clone` has just created. The
    // offer adds the key's options to the command configured there.
    let _offer = if ssh_key::scoped() {
        let created = gix::open_opts(dest, clone_open_options())
            .map_err(|e| git_error(e, repo, &supplied))?;
        match ssh_key::for_remote(&created, repo).map_err(SourceError::Git)? {
            Some(held) => {
                let offer = held.offer(&created).map_err(SourceError::Git)?;
                clone = clone.with_in_memory_config_overrides(offer.overrides.clone());
                Some(offer)
            }
            None => None,
        }
    } else {
        None
    };
    if credentials::resolving() {
        // gix retries a clone whose remote has a `refs/heads/HEAD` branch on a
        // second connection it does not let Toha prepare, so that connection
        // would skip the resolver. Excluding the branch removes the retry. Setting
        // the remote here also means gix leaves tags to it, so ask for all tags,
        // as a clone does without this setting.
        clone = clone.configure_remote(|remote| {
            Ok(remote
                .with_refspecs(["^refs/heads/HEAD"], gix::remote::Direction::Fetch)?
                .with_fetch_tags(gix::remote::fetch::Tags::All))
        });
    }
    if let Some(reference) = reference {
        clone = if kind == RefKind::Commit {
            clone
                .with_revision(Some(resolved_commit.as_deref().unwrap()))
                .map_err(|e| git_error(e, repo, &supplied))?
        } else {
            clone
                .with_ref_name(Some(reference.as_str()))
                .map_err(|e| git_error(e, repo, &supplied))?
        };
    }
    let fetched = clone.fetch_then_checkout(gix::progress::Discard, interrupt);
    if interrupt.load(Ordering::SeqCst) {
        return Err(SourceError::Stopped);
    }
    let (mut checkout, _) = fetched.map_err(|e| git_error(e, repo, &supplied))?;
    let checked_out = checkout.main_worktree(gix::progress::Discard, interrupt);
    // An interrupted checkout still returns `Ok`.
    if interrupt.load(Ordering::SeqCst) {
        return Err(SourceError::Stopped);
    }
    let (checkout_repo, _) = checked_out.map_err(|e| git_error(e, repo, &supplied))?;
    if kind == RefKind::Branch
        && reference.as_ref().is_some_and(|r| {
            checkout_repo
                .find_reference(&format!("refs/tags/{r}"))
                .is_ok()
        })
    {
        kind = RefKind::Tag;
    }
    let commit = checkout_repo
        .head_id()
        .map_err(|e| git_error(e, repo, &supplied))?
        .to_string();
    Ok(Fetched {
        commit,
        reference_kind: kind,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_file_url_uses_platform_path() {
        assert_eq!(windows_local_git_path("/C:/sample/repo"), "C:/sample/repo");
        let expected = if cfg!(windows) {
            "C:/sample/repo"
        } else {
            "/sample/repo"
        };
        let input = if cfg!(windows) {
            "file:///C:/sample/repo"
        } else {
            "file:///sample/repo"
        };
        assert_eq!(local_git_path(input), Some(expected));
    }
    #[test]
    fn parse_order_and_formal_names() {
        let root = tempfile::tempdir().unwrap();
        let hosts = Hosts::from([
            ("gh".into(), "https://github.com".into()),
            ("local".into(), format!("file://{}", root.path().display())),
        ]);
        let cwd = root.path();
        let folder = cwd.join("folder");
        std::fs::create_dir(&folder).unwrap();
        let table = [
            ("https://github.com/a/b.git@main#sub", "gh:a/b@main#sub"),
            ("git@github.com:a/b.git@main#sub", "gh:a/b@main#sub"),
            ("https://user@github.com/a/b", "gh:a/b"),
            ("https://user@github.com/a/b@v1#p", "gh:a/b@v1#p"),
            ("git@github.com:a/b@main", "gh:a/b@main"),
            ("https://token@else.invalid/a/b", "https://else.invalid/a/b"),
            ("gh:a/b@main#sub", "gh:a/b@main#sub"),
            ("local:a/b#sub", "local:a/b#sub"),
            ("https://else.invalid/a/b.git", "https://else.invalid/a/b"),
            (
                "https://github.com.evil/a/b.git",
                "https://github.com.evil/a/b",
            ),
        ];
        for (input, formal) in table {
            let address = parse(input, &hosts, cwd, cwd).unwrap();
            assert_eq!(address.formal_name(&hosts), formal, "{input}");
            if let Address::Git {
                repo, reference, ..
            } = &address
            {
                assert!(!without_userinfo(repo).contains("token"));
                assert!(
                    !cache_path(cwd, &address, &hosts, "abc")
                        .to_string_lossy()
                        .contains("token")
                );
                if input.contains("@v1") {
                    assert_eq!(reference.as_deref(), Some("v1"));
                }
            }
        }
        assert!(matches!(
            parse("./folder", &hosts, cwd, cwd).unwrap(),
            Address::Folder(_)
        ));
        let canonical = folder.canonicalize().unwrap();
        let parsed = parse(canonical.to_str().unwrap(), &hosts, cwd, cwd).unwrap();
        assert!(matches!(parsed, Address::Folder(path) if path == canonical));
        let names: &[&str] = if cfg!(windows) {
            &["C:relative"]
        } else {
            &[r"\\?\C:\sample", r"\\server\share", "C:relative"]
        };
        for &name in names {
            assert_eq!(
                parse(name, &hosts, cwd, cwd).unwrap(),
                Address::Name(name.into())
            );
        }
        assert_eq!(
            parse("plain", &hosts, cwd, cwd).unwrap(),
            Address::Name("plain".into())
        );
        assert!(parse("gh:invalid", &hosts, cwd, cwd).is_err());
        let address = parse("gh:a/b#sub", &hosts, cwd, cwd).unwrap();
        assert_eq!(
            cache_path(cwd, &address, &hosts, "abc"),
            cwd.join("sources/gh_a_b/abc")
        );
        let repo = "https://token@host.invalid/o/r";
        let error = git_error(
            format!("clone failed for {repo}"),
            repo,
            &Supplied::default(),
        )
        .to_string();
        assert!(!error.contains("token"));
        assert!(error.contains("https://host.invalid/o/r"));
    }
    #[test]
    fn a_url_with_a_password_is_refused_and_a_user_alone_is_not() {
        for url in [
            "https://user:secret@host.invalid/o/r.git",
            "https://user:s%40cr%2Fet@host.invalid/o/r.git",
            "https://user:@host.invalid/o/r.git",
            "https://user:p@ss@host.invalid/o/r.git",
            "https://user:p#ss@host.invalid/o/r.git",
            "https://user:secret@host.invalid",
            "HTTP://user:secret@host.invalid:8080/r",
            "ssh://git:secret@host.invalid/o/r.git",
        ] {
            assert!(has_password(url), "{url}");
        }
        for url in [
            "https://host.invalid/o/r.git",
            "https://token@host.invalid/o/r.git",
            "https://host.invalid:8443/o/r.git",
            "ssh://git@host.invalid:22/o/r.git",
            "git@host.invalid:o/r.git",
            "https://host.invalid/o/r@a:b",
            "file:///home/a:b@c/repo",
            "../relative/repo",
        ] {
            assert!(!has_password(url), "{url}");
        }
    }
    #[test]
    fn a_password_refusal_precedes_fetch_destination_checks() {
        let root = tempfile::tempdir().unwrap();
        let address = Address::Git {
            repo: "https://user:secret@host.invalid/o/r.git".into(),
            reference: None,
            path: None,
        };
        let existing = root.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("sentinel"), "untouched").unwrap();
        let missing = root.path().join("missing");
        crate::stop::with_stop_signal(crate::stop::StopSignal::new(), || {
            for dest in [missing.join("clone"), existing.clone()] {
                let error = fetch(&address, &dest).unwrap_err();
                assert!(
                    matches!(error, SourceError::Address(ref message) if message == PASSWORD_REFUSED),
                    "{error:?}"
                );
            }
        });
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&existing).unwrap().count(), 1);
        assert_eq!(
            std::fs::read_to_string(existing.join("sentinel")).unwrap(),
            "untouched"
        );
    }
    #[test]
    fn a_password_refusal_precedes_temporary_fetch_directory_creation() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("missing");
        let address = Address::Git {
            repo: "https://user:secret@host.invalid/o/r.git".into(),
            reference: None,
            path: None,
        };
        let refuse = || {
            let error = fetch_temporary(&address, &parent, ".tmp").unwrap_err();
            assert!(
                matches!(error, SourceError::Address(ref message) if message == PASSWORD_REFUSED),
                "{error:?}"
            );
            assert!(!parent.exists());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        };
        refuse();
        crate::stop::with_stop_signal(crate::stop::StopSignal::new(), refuse);
    }
    #[test]
    fn an_address_with_a_password_is_refused_without_naming_it() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path();
        let hosts = Hosts::from([("open".into(), "https://user:secret@host.invalid".into())]);
        for address in [
            "https://user:secret@host.invalid/o/r.git",
            "https://user:se#cret@host.invalid/o/r.git@main#sub",
            "open:o/r",
        ] {
            let error = parse(address, &hosts, cwd, cwd).unwrap_err().to_string();
            assert!(error.contains("with a password is not accepted"), "{error}");
            assert!(!error.contains("secret"), "{error}");
            assert!(!error.contains("cret"), "{error}");
        }
        let written = Address::Git {
            repo: "https://user:secret@host.invalid/o/r.git".into(),
            reference: None,
            path: None,
        };
        let dest = cwd.join("clone");
        let error = fetch(&written, &dest).unwrap_err().to_string();
        assert!(error.contains("with a password is not accepted"), "{error}");
        assert!(!error.contains("secret"), "{error}");
        assert!(!dest.exists());
    }
}
