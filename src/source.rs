//! Template addresses and immutable Git fetches.
use base64::Engine;
use gix::bstr::ByteSlice;
use indexmap::IndexMap;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
mod credentials;
mod failure;
pub(crate) use credentials::Supplied;
pub use credentials::{
    Credential, CredentialRequest, CredentialResolver, with_credential_resolver,
};
pub(crate) use failure::{Operation, message as failure_message};
pub type Hosts = IndexMap<String, String>;
/// A template's address. Its `Debug` shows a Git address without user
/// information.
#[derive(Clone, PartialEq, Eq)]
pub enum Address {
    Git {
        repo: String,
        reference: Option<String>,
        path: Option<String>,
    },
    Folder(PathBuf),
    Name(String),
}
impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git {
                repo,
                reference,
                path,
            } => f
                .debug_struct("Git")
                .field("repo", &without_userinfo(repo))
                .field("reference", reference)
                .field("path", path)
                .finish(),
            Self::Folder(folder) => f.debug_tuple("Folder").field(folder).finish(),
            Self::Name(name) => f.debug_tuple("Name").field(name).finish(),
        }
    }
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
}
fn split_git(value: &str) -> Result<(&str, Option<String>, Option<String>), SourceError> {
    let hash = template_path_start(value);
    let (before_path, path) = match hash {
        Some(hash) => (&value[..hash], Some(value[hash + 1..].to_string())),
        None => (value, None),
    };
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
/// The `#` that starts the template path of the git address `value`.
///
/// It is the first `#`, unless that `#` sits in the user information of a
/// URL, as in `https://user:pa#ss@host/repository`; the template path then
/// starts at the first `#` after the authority's last `@`. gix ends an HTTP
/// authority at a `#`, so there the `#` is in the user information only when
/// gix rejects the text before it; for any other scheme a `#` is authority
/// text, in the user information when a `@` follows it in the authority.
fn template_path_start(value: &str) -> Option<usize> {
    let first = value.find('#')?;
    let Some(scheme) = value.find("://").filter(|&scheme| scheme < first) else {
        return Some(first);
    };
    let authority = scheme + 3;
    let end = value[authority..]
        .find('/')
        .map_or(value.len(), |slash| authority + slash);
    if first > end {
        return Some(first);
    }
    let in_userinfo = if is_http(&value[..scheme]) {
        gix::url::parse(gix::bstr::BStr::new(&value[..first])).is_err()
    } else {
        value[first..end].contains('@')
    };
    if !in_userinfo {
        return Some(first);
    }
    let after_userinfo = value[authority..end]
        .rfind('@')
        .map_or(authority, |at| authority + at + 1);
    value[after_userinfo..]
        .find('#')
        .map(|hash| after_userinfo + hash)
}
pub fn parse(arg: &str, hosts: &Hosts, cwd: &Path, home: &Path) -> Result<Address, SourceError> {
    if arg.contains("://") || arg.starts_with("git@") {
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
        return Ok(Address::Git {
            repo: format!("{}/{}", base.trim_end_matches('/'), repo),
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
/// `repo` without its user information: the form Toha shows, records and keys
/// registry and cache entries by.
///
/// It reads the address's structure and never looks for a password's text.
/// In `scheme://authority/path` the authority ends at the first `/`, and the
/// user information is everything before the authority's last `@`; text with
/// a `@` in front of a `://` that is not a scheme can only be user
/// information. A scp-like `user@host:path` is read as git reads it (see
/// [`scp`]); `git@host:path` becomes `ssh://host/path`. A remote-helper
/// address `helper::address` keeps its helper and loses the user information
/// of its address. A `/` cannot appear unencoded in user information, so text
/// after one is the host or path, as it is for gix and git.
pub fn without_userinfo(repo: &str) -> String {
    if let Some((helper, address)) = helper_address(repo) {
        return format!("{helper}::{}", without_userinfo(address));
    }
    if let Some((scheme, rest)) = repo.split_once("://") {
        if is_scheme(scheme) {
            let authority = &rest[..authority_end(scheme, repo, rest)];
            return match authority.rfind('@') {
                Some(at) => format!("{scheme}://{}", &rest[at + 1..]),
                None => repo.to_owned(),
            };
        }
        return match scheme.rfind('@') {
            Some(at) => without_userinfo(&repo[at + 1..]),
            None => repo.to_owned(),
        };
    }
    match scp(repo) {
        Some((Some("git"), host, path)) => format!("ssh://{host}/{path}"),
        Some((Some(_), host, path)) => format!("{host}:{path}"),
        // A scp-like address without a path, as Toha wrote it before.
        None if repo.starts_with("git@") && !repo[4..].contains('@') => {
            format!("ssh://{}", &repo[4..])
        }
        _ => repo.to_owned(),
    }
}
/// `remote` without the user information that could carry a credential, for
/// a remote Toha saves. An SSH address keeps its user, which names the
/// account SSH logs in to and is not a secret, and loses any password after
/// it; a scp-like address cannot hold a password, so it stays as it is; every
/// other address loses all of its user information, as the user of an HTTPS
/// address can be a token. The user information is found as
/// [`without_userinfo`] finds it, and nothing else of the address changes.
pub(crate) fn without_credentials(remote: &str) -> String {
    if let Some((helper, address)) = helper_address(remote) {
        return format!("{helper}::{}", without_credentials(address));
    }
    if let Some((scheme, rest)) = remote.split_once("://")
        && matches!(
            scheme.to_ascii_lowercase().as_str(),
            "ssh" | "git+ssh" | "ssh+git"
        )
    {
        let authority = rest.find('/').map_or(rest, |slash| &rest[..slash]);
        let Some(at) = authority.rfind('@') else {
            return remote.to_owned();
        };
        let user = rest[..at].split(':').next().unwrap_or_default();
        let user = if user.is_empty() {
            String::new()
        } else {
            format!("{user}@")
        };
        return format!("{scheme}://{user}{}", &rest[at + 1..]);
    }
    if remote.contains("://") {
        return without_userinfo(remote);
    }
    remote.to_owned()
}
/// Where the authority of `url`, whose part after `scheme://` is `rest`, ends.
/// An HTTP authority ends at the first `/`, `?` or `#`, as gix and RFC 3986
/// read it, when gix parses the URL, so a `@` in a query is not taken for
/// user information. Any other authority, and that of an HTTP URL gix
/// rejects, such as one whose password holds a `?` or `#`, ends at the first
/// `/`, so that all the text before the host's `@` is treated as user
/// information.
fn authority_end(scheme: &str, url: &str, rest: &str) -> usize {
    let slash = rest.find('/').unwrap_or(rest.len());
    // gix reads a `?` or `#` as the end of the authority only for these two
    // schemes, spelled in lowercase; `HTTPS` is a helper scheme to it, whose
    // authority runs to the first `/`.
    if !is_http(scheme) {
        return slash;
    }
    let Ok(parsed) = gix::url::parse(gix::bstr::BStr::new(url)) else {
        return slash;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    if (parsed.user().is_some() || parsed.password().is_some()) && !rest[..end].contains('@') {
        // gix found user information this boundary would miss.
        return slash;
    }
    end
}
/// Whether gix reads `scheme` as HTTP, where a `?` or `#` ends the authority.
fn is_http(scheme: &str) -> bool {
    scheme == "http" || scheme == "https"
}
/// The helper and address of a remote-helper address `helper::address`.
fn helper_address(repo: &str) -> Option<(&str, &str)> {
    repo.split_once("::")
        .filter(|(helper, _)| is_name(helper, b"+-._"))
}
/// Whether `text` is a URL scheme: a letter, then letters, digits, `+`, `-`
/// and `.`.
fn is_scheme(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic()) && is_name(text, b"+-.")
}
/// The user, host and path of a scp-like address `[user@]host:path`, read as
/// git reads it: the host ends at the first `:`, when no `/` comes before it,
/// and the user is everything before the last `@` in front of the host. A
/// `:` or `@` after that first `:` belongs to the path, and the user cannot
/// hold a `:`, so a scp-like address has no password.
fn scp(address: &str) -> Option<(Option<&str>, &str, &str)> {
    let colon = address.find(':')?;
    let authority = &address[..colon];
    if authority.contains('/') {
        return None;
    }
    let (user, host) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };
    Some((user, host, &address[colon + 1..]))
}
/// Whether `text` is a non-empty run of ASCII letters, digits and `extra`.
fn is_name(text: &str, extra: &[u8]) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
}
/// Remove the address from what the clone at `repository` saved of it. A
/// clone saves the address it was given as its remote and names it in the
/// `clone: from <address>` message of its reflogs, as `git clone` does, and
/// Toha keeps its clones in its cache and data folders, where a password in
/// the address would stay on disk. Toha never fetches through a saved remote
/// and never reads a clone's reflogs, so it saves every remote URL without
/// its user information and removes the reflogs.
fn forget_credentials(
    repository: &gix::Repository,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let logs = repository.git_dir().join("logs");
    if logs.exists() {
        std::fs::remove_dir_all(&logs)?;
    }
    let path = repository.common_dir().join("config");
    let mut file =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)?;
    let saved: Vec<(gix::bstr::BString, String)> = file
        .sections_by_name("remote")
        .into_iter()
        .flatten()
        .filter_map(|section| {
            let name = section.header().subsection_name()?.to_owned();
            let url = section.value("url")?.to_str_lossy().into_owned();
            Some((name, url))
        })
        .collect();
    let mut changed = false;
    for (name, url) in saved {
        let safe = without_userinfo(&url);
        if safe != url {
            file.set_raw_value_by("remote", Some(name.as_ref()), "url", safe.as_str())?;
            changed = true;
        }
    }
    if changed {
        let mut out = std::fs::File::create(&path)?;
        file.write_to(&mut out)?;
    }
    Ok(())
}
/// The fetch failure for `error`: the resolver's refusal when it refused,
/// otherwise Toha's own message, which never holds gix's text.
fn git_error(
    error: &(dyn std::error::Error + 'static),
    repo: &str,
    operation: Operation,
    supplied: &Supplied,
) -> SourceError {
    SourceError::Git(
        supplied
            .refusal()
            .unwrap_or_else(|| failure::message(repo, operation, error)),
    )
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
        let remote = gix::open(local).map_err(|e| opening(&e, repo))?;
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
fn expand_commit(repo: &str, short: &str, dest: &Path) -> Result<String, SourceError> {
    if short.len() == 40 {
        return Ok(short.into());
    }
    let local = local_git_path(repo).unwrap_or(repo);
    if repo.starts_with("file://") {
        let remote = gix::open(local).map_err(|e| opening(&e, repo))?;
        return resolve_commit(&remote, short, repo);
    }
    let parent = dest
        .parent()
        .ok_or_else(|| SourceError::Address("destination has no parent".into()))?;
    let temporary = tempfile::tempdir_in(parent).map_err(|e| SourceError::Io(parent.into(), e))?;
    let lookup = temporary.path().join("lookup");
    fetch(
        &Address::Git {
            repo: repo.into(),
            reference: None,
            path: None,
        },
        &lookup,
    )?;
    let fetched = gix::open(&lookup).map_err(|e| opening(&e, repo))?;
    resolve_commit(&fetched, short, repo)
}
/// The failure to open the repository of `repo` or a clone of it.
fn opening(error: &(dyn std::error::Error + 'static), repo: &str) -> SourceError {
    SourceError::Git(failure::message(repo, Operation::Open, error))
}
/// The full id of the commit `short` names in `repository`, a clone of `repo`.
fn resolve_commit(
    repository: &gix::Repository,
    short: &str,
    repo: &str,
) -> Result<String, SourceError> {
    repository
        .rev_parse_single(short)
        .map(|id| id.to_string())
        .map_err(|_| {
            SourceError::Git(format!(
                "could not resolve the commit '{short}' in '{}'",
                without_userinfo(repo)
            ))
        })
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    supplied.attach(connection);
    configure_http(connection, native_roots)
}
/// Set the HTTP options of a connection. curl's verbose output is off, as it
/// writes the request headers to stderr, with the credentials of the address,
/// the Git configuration or a resolver among them. An HTTPS connection is
/// pointed at the platform's trusted roots: system curl backends already use
/// platform trust, and bundled rustls needs a CA bundle when Git has not
/// supplied one.
fn configure_http(
    connection: &mut Connection<'_, '_, '_>,
    native_roots: &mut NativeRoots,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let rustls = curl::Version::get()
        .ssl_version()
        .is_some_and(|backend| backend.starts_with("rustls"));
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
        http.verbose = false;
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
/// A remote branch named `HEAD`, which a clone fetches only when selected.
const HEAD_BRANCH: &str = "refs/heads/HEAD";
pub fn fetch(address: &Address, dest: &Path) -> Result<Fetched, SourceError> {
    let Address::Git {
        repo, reference, ..
    } = address
    else {
        return Err(SourceError::Address("a git address is required".into()));
    };
    let mut kind = reference_kind(repo, reference.as_deref())?;
    if dest.exists() {
        return Err(SourceError::Address(format!(
            "destination exists: {}",
            dest.display()
        )));
    }
    let resolved_commit = if kind == RefKind::Commit {
        Some(expand_commit(repo, reference.as_deref().unwrap(), dest)?)
    } else {
        None
    };
    let mut native_roots = None;
    let supplied = Supplied::default();
    let attach = supplied.clone();
    let mut clone = gix::prepare_clone(repo.as_str(), dest)
        .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?
        .configure_connection(move |connection| {
            prepare_connection(connection, &mut native_roots, &attach)
        });
    // gix retries a clone whose remote has a `refs/heads/HEAD` branch, which
    // clashes with the clone's own `HEAD` tracking ref, on a second
    // connection it does not let Toha prepare. That connection would skip the
    // resolver and turn curl's verbose output back on, which writes the
    // request headers with their credentials. Excluding the branch removes
    // the retry. Setting the remote here also means gix leaves tags to it, so
    // ask for all tags, as a clone does without this setting. An explicit
    // selection of that branch fetches it as a revision: on one connection,
    // without the clashing tracking ref.
    let head_branch = reference.as_deref() == Some(HEAD_BRANCH);
    if !head_branch {
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
                .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?
        } else if head_branch {
            clone
                .with_revision(Some(HEAD_BRANCH))
                .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?
        } else {
            clone
                .with_ref_name(Some(reference.as_str()))
                .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?
        };
    }
    let (mut checkout, _) = clone
        .fetch_then_checkout(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?;
    let (checkout_repo, _) = checkout
        .main_worktree(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?;
    forget_credentials(&checkout_repo)
        .map_err(|e| git_error(&*e, repo, Operation::Fetch, &supplied))?;
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
        .map_err(|e| git_error(&e, repo, Operation::Fetch, &supplied))?
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
            (
                "https://user:token@else.invalid/a/b",
                "https://else.invalid/a/b",
            ),
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
        let repo = "https://user:token@host.invalid/o/r";
        let error = std::io::Error::other(format!("clone failed for {repo}"));
        let error = git_error(&error, repo, Operation::Fetch, &Supplied::default());
        assert_eq!(
            error.to_string(),
            "git fetch failed: could not fetch 'https://host.invalid/o/r'"
        );
    }
    /// Passwords that are spelled differently in a URL, in gix's rewrite of
    /// it and in Rust's escaped form: reserved characters raw and encoded,
    /// encodings gix decodes and writes back in another case or not at all,
    /// non-ASCII text, and control characters next to non-ASCII text.
    const UNUSUAL_PASSWORDS: [&str; 14] = [
        "plain-secret",
        "p@ss-secret",
        "p%40ss-secret",
        "p%2Ass-secret",
        "caf%c3%a9-secret",
        "caf\u{e9}-secret",
        "sp%20ace-secret",
        "co:lon-secret",
        "que?ry-secret",
        "fra#gment-secret",
        "p\tss-secret",
        "caf\u{e9}\u{1}\t-secret",
        "\u{7f}caf\u{e9}-secret",
        "p%zz-secret",
    ];
    #[test]
    fn userinfo_is_removed_from_every_address_form() {
        for password in UNUSUAL_PASSWORDS {
            let table = [
                (
                    format!("https://user:{password}@host.invalid/o/r.git"),
                    "https://host.invalid/o/r.git",
                ),
                (
                    format!("https://user:{password}@[::1]:8443/o/r"),
                    "https://[::1]:8443/o/r",
                ),
                (
                    format!("http://user:{password}@host.invalid"),
                    "http://host.invalid",
                ),
                (
                    format!("git://user:{password}@host.invalid/r"),
                    "git://host.invalid/r",
                ),
                (
                    format!("ssh://user:{password}@host.invalid/r"),
                    "ssh://host.invalid/r",
                ),
                (
                    format!("git+ssh://user:{password}@host.invalid/r"),
                    "git+ssh://host.invalid/r",
                ),
                (
                    format!("file://user:{password}@host.invalid/tmp/r"),
                    "file://host.invalid/tmp/r",
                ),
                (
                    format!("persistent-https::https://user:{password}@host.invalid/r"),
                    "persistent-https::https://host.invalid/r",
                ),
                (
                    format!("user:{password}@https://host.invalid/r"),
                    "https://host.invalid/r",
                ),
            ];
            for (url, expected) in table {
                assert_eq!(without_userinfo(&url), expected, "{url:?}");
            }
        }
    }
    #[test]
    fn addresses_without_userinfo_keep_their_spelling() {
        let table = [
            ("https://github.com/a/b.git", "https://github.com/a/b.git"),
            ("https://GitHub.com/A/B%20C", "https://GitHub.com/A/B%20C"),
            ("https://user@github.com/a/b", "https://github.com/a/b"),
            ("https://host.invalid/a@b/c", "https://host.invalid/a@b/c"),
            (
                "https://host.invalid?owner=alice@example.invalid/r.git",
                "https://host.invalid?owner=alice@example.invalid/r.git",
            ),
            (
                "https://host.invalid#a@example.invalid/r.git",
                "https://host.invalid#a@example.invalid/r.git",
            ),
            ("git@github.com:a/b.git", "ssh://github.com/a/b.git"),
            ("git@github.com", "ssh://github.com"),
            ("bob@host.invalid:repo.git", "host.invalid:repo.git"),
            ("bob@host.invalid:repo@draft", "host.invalid:repo@draft"),
            (
                "git@github.com:a/b@draft.git",
                "ssh://github.com/a/b@draft.git",
            ),
            ("host.invalid:repo@draft", "host.invalid:repo@draft"),
            ("file:///tmp/a@b.git", "file:///tmp/a@b.git"),
            ("/tmp/a@b:c", "/tmp/a@b:c"),
            ("../rel@x.git", "../rel@x.git"),
            ("rel@x.git", "rel@x.git"),
            ("gh:a/b", "gh:a/b"),
            ("ext::sh -c x", "ext::sh -c x"),
        ];
        for (url, expected) in table {
            assert_eq!(without_userinfo(url), expected, "{url:?}");
            assert_eq!(without_userinfo(expected), expected, "{expected:?}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn a_destination_toha_cannot_write_is_not_an_authentication_failure() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        if std::fs::create_dir(locked.join("probe")).is_ok() {
            // A superuser writes anyway; the case cannot arise.
            return;
        }
        let address = Address::Git {
            repo: "https://user:p%40ss-secret@example.invalid/o/r.git".into(),
            reference: None,
            path: None,
        };
        let error = fetch(&address, &locked.join("clone"))
            .unwrap_err()
            .to_string();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            error,
            format!(
                "git fetch failed: could not fetch 'https://example.invalid/o/r.git': {}",
                std::io::Error::from_raw_os_error(13)
            )
        );
    }
    #[test]
    fn a_saved_remote_keeps_an_ssh_user_and_no_credential() {
        for password in UNUSUAL_PASSWORDS {
            let table = [
                (
                    format!("https://user:{password}@host.invalid/o/r.git"),
                    "https://host.invalid/o/r.git".to_owned(),
                ),
                (
                    format!("git://user:{password}@host.invalid/r"),
                    "git://host.invalid/r".to_owned(),
                ),
                (
                    format!("ssh://git:{password}@host.invalid:2222/o/r.git"),
                    "ssh://git@host.invalid:2222/o/r.git".to_owned(),
                ),
                (
                    format!("persistent-https::https://user:{password}@host.invalid/r"),
                    "persistent-https::https://host.invalid/r".to_owned(),
                ),
            ];
            for (remote, kept) in table {
                assert_eq!(without_credentials(&remote), kept, "{remote:?}");
            }
        }
        // A token in the user position cannot hold an unencoded `?` or `#`:
        // `https://token-a?b@host/r` is the host `token-a` with a query, for
        // gix as for RFC 3986, so it keeps its spelling.
        for token in UNUSUAL_PASSWORDS
            .iter()
            .filter(|token| !token.contains(['?', '#']))
        {
            assert_eq!(
                without_credentials(&format!("https://{token}@host.invalid/o/r.git")),
                "https://host.invalid/o/r.git",
                "{token:?}"
            );
        }
        assert_eq!(
            without_credentials("https://token-a?b@host.invalid/o/r.git"),
            "https://token-a?b@host.invalid/o/r.git"
        );
        for remote in [
            "git@github.com:o/r.git",
            "git@github.com:o/r@draft.git",
            "ssh://git@github.com/o/r.git",
            "https://github.com/o/r.git",
            "../relative@r.git",
            "/tmp/a@b:c",
        ] {
            assert_eq!(without_credentials(remote), remote);
        }
    }
    #[test]
    fn a_scheme_in_another_case_loses_its_user_information_too() {
        // gix reads `HTTPS` as a helper scheme, whose authority runs to the
        // first `/`, and finds the password even with a `?` or `#` in it.
        for scheme in ["HTTPS", "HtTpS", "HTTP"] {
            for password in UNUSUAL_PASSWORDS {
                let url = format!("{scheme}://user:{password}@host.invalid/o/r.git");
                let kept = format!("{scheme}://host.invalid/o/r.git");
                assert_eq!(without_userinfo(&url), kept, "{url:?}");
                assert_eq!(without_credentials(&url), kept, "{url:?}");
            }
        }
        let hosts = Hosts::new();
        let cwd = Path::new("/");
        let address = parse(
            "HTTPS://user:pa#ss-secret@host.invalid/o/r@v1#sub",
            &hosts,
            cwd,
            cwd,
        )
        .unwrap();
        assert_eq!(
            address.formal_name(&hosts),
            "HTTPS://host.invalid/o/r@v1#sub"
        );
    }
    #[test]
    fn a_scp_address_is_read_as_git_reads_it() {
        // The host ends at the first `:`; a `:` or `@` after it is the path's.
        let table = [
            (
                "git@host.invalid:repo@draft:branch",
                "ssh://host.invalid/repo@draft:branch",
                "git@host.invalid:repo@draft:branch",
            ),
            (
                "secret-token@host.invalid:a@b:c@d",
                "host.invalid:a@b:c@d",
                "secret-token@host.invalid:a@b:c@d",
            ),
            (
                "host.invalid:repo@draft:branch",
                "host.invalid:repo@draft:branch",
                "host.invalid:repo@draft:branch",
            ),
        ];
        for (address, shown, kept) in table {
            assert_eq!(without_userinfo(address), shown, "{address:?}");
            assert_eq!(without_credentials(address), kept, "{address:?}");
            assert_eq!(scp(address).map(|(_, host, _)| host), Some("host.invalid"));
        }
    }
    #[test]
    fn a_scp_path_with_an_at_sign_keeps_its_userinfo_apart() {
        for user in ["secret-token", "u%40secret", "p@ss-secret"] {
            for path in ["repo@draft", "o/r@v1.git", "repo", "repo@draft:branch"] {
                let address = format!("{user}@host.invalid:{path}");
                assert_eq!(
                    without_userinfo(&address),
                    format!("host.invalid:{path}"),
                    "{address:?}"
                );
            }
        }
    }
    #[test]
    fn the_template_path_starts_at_the_first_hash_after_a_host() {
        let hosts = Hosts::new();
        let cwd = Path::new("/");
        let table = [
            (
                "https://host.invalid#user@team/sub",
                ("https://host.invalid", None, Some("user@team/sub")),
            ),
            (
                "https://host.invalid:8443#sub",
                ("https://host.invalid:8443", None, Some("sub")),
            ),
            (
                "https://[::1]:8443#sub",
                ("https://[::1]:8443", None, Some("sub")),
            ),
            (
                "https://host.invalid:#user@team/sub",
                ("https://host.invalid:", None, Some("user@team/sub")),
            ),
            (
                "https://[fe80::1%25en-0]#user@team/sub",
                ("https://[fe80::1%25en-0]", None, Some("user@team/sub")),
            ),
            (
                "https://user@host.invalid#sub",
                ("https://user@host.invalid", None, Some("sub")),
            ),
            (
                "https://host.invalid/o/r@v1#a#b",
                ("https://host.invalid/o/r", Some("v1"), Some("a#b")),
            ),
            (
                "https://user:pa#ss-secret@host.invalid/o/r",
                ("https://user:pa#ss-secret@host.invalid/o/r", None, None),
            ),
            (
                "https://user:pa#ss-secret@host.invalid/o/r@v1#sub",
                (
                    "https://user:pa#ss-secret@host.invalid/o/r",
                    Some("v1"),
                    Some("sub"),
                ),
            ),
        ];
        for (input, (repo, reference, path)) in table {
            let parsed = parse(input, &hosts, cwd, cwd).unwrap();
            assert_eq!(
                parsed,
                Address::Git {
                    repo: repo.into(),
                    reference: reference.map(Into::into),
                    path: path.map(Into::into),
                },
                "{input:?}"
            );
        }
        let address = parse("https://host.invalid#user@team/sub", &hosts, cwd, cwd).unwrap();
        assert_eq!(
            address.formal_name(&hosts),
            "https://host.invalid#user@team/sub"
        );
    }
    #[test]
    fn formal_names_and_cache_paths_hold_no_unusual_password() {
        let hosts = Hosts::from([("gh".into(), "https://github.com".into())]);
        let cwd = Path::new("/");
        for password in UNUSUAL_PASSWORDS {
            for input in [
                format!("https://user:{password}@github.com/a/b@v1#sub"),
                format!("https://user:{password}@else.invalid/a/b.git"),
            ] {
                let address = parse(&input, &hosts, cwd, cwd).unwrap();
                let formal = address.formal_name(&hosts);
                let cache = cache_path(cwd, &address, &hosts, "abc");
                for shown in [
                    formal,
                    cache.to_string_lossy().into_owned(),
                    format!("{address:?}"),
                ] {
                    assert!(!shown.contains("secret"), "{input:?} shows {shown:?}");
                }
            }
        }
    }
}
