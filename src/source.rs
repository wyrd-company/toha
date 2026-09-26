//! Template addresses and immutable Git fetches.
use indexmap::IndexMap;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
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
        let (repo, reference, path) = split_git(arg)?;
        return Ok(Address::Git {
            repo: repo.into(),
            reference,
            path,
        });
    }
    if let Some((prefix, rest)) = arg.split_once(':') {
        if let Some(base) = hosts.get(prefix) {
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
    }
    let drive = arg
        .as_bytes()
        .get(0..2)
        .is_some_and(|v| v[0].is_ascii_alphabetic() && v[1] == b':');
    if arg.starts_with('.') || arg.starts_with('/') || arg.starts_with('~') || drive {
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
fn git_error(error: impl std::fmt::Display, repo: &str) -> SourceError {
    let safe = without_userinfo(repo);
    let mut message = error.to_string().replace(repo, &safe);
    if let Some(scheme) = repo.find("://") {
        let start = scheme + 3;
        let end = repo[start..]
            .find('/')
            .map_or(repo.len(), |slash| start + slash);
        if let Some(at) = repo[start..end].rfind('@') {
            message = message.replace(&repo[start..start + at + 1], "");
        }
    }
    SourceError::Git(message)
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
    if let Some(local) = repo.strip_prefix("file://") {
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
fn expand_commit(repo: &str, short: &str, dest: &Path) -> Result<String, SourceError> {
    if short.len() == 40 {
        return Ok(short.into());
    }
    let local = repo.strip_prefix("file://").unwrap_or(repo);
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
    fetch(
        &Address::Git {
            repo: repo.into(),
            reference: None,
            path: None,
        },
        &lookup,
    )?;
    gix::open(&lookup)
        .map_err(|e| SourceError::Git(e.to_string()))?
        .rev_parse_single(short)
        .map(|id| id.to_string())
        .map_err(|e| SourceError::Git(e.to_string()))
}
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
    let mut clone = gix::prepare_clone(repo.as_str(), dest).map_err(|e| git_error(e, repo))?;
    if let Some(reference) = reference {
        clone = if kind == RefKind::Commit {
            clone
                .with_revision(Some(resolved_commit.as_deref().unwrap()))
                .map_err(|e| git_error(e, repo))?
        } else {
            clone
                .with_ref_name(Some(reference.as_str()))
                .map_err(|e| git_error(e, repo))?
        };
    }
    let (mut checkout, _) = clone
        .fetch_then_checkout(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| git_error(e, repo))?;
    let (checkout_repo, _) = checkout
        .main_worktree(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| git_error(e, repo))?;
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
        .map_err(|e| git_error(e, repo))?
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
        let error = git_error(format!("clone failed for {repo}"), repo).to_string();
        assert!(!error.contains("token"));
        assert!(error.contains("https://host.invalid/o/r"));
    }
}
