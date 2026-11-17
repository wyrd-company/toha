// ---
// relationships:
//   implements: architecture
// ---
//! What Toha reports when an operation on a Git remote fails.
//!
//! Toha writes these messages itself, as git does. A message names the remote
//! by its address without user information and gives a reason Toha chose from
//! the error's type. It never holds the text of a gix error: gix writes the
//! remote's URL, with whatever credentials it carries and in encodings of its
//! own, and the text a remote sends back.
use gix::bstr::ByteSlice;
use std::{error::Error, io::ErrorKind};

/// What Toha was doing with a remote when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    /// Fetching a template source.
    Fetch,
    /// Listing a remote's references for a `git` question.
    List,
    /// Opening a local repository a `file:` address names.
    Open,
    /// Saving the remote of a `git` answer in a new repository.
    Save,
}

/// Toha's message for `error`, the failure of `operation` on the remote `url`.
pub(crate) fn message(url: &str, operation: Operation, error: &(dyn Error + 'static)) -> String {
    let url = super::without_userinfo(url);
    match reason(&url, error) {
        Reason::Authentication => format!("authentication failed for '{url}'"),
        Reason::CleartextCredentials => {
            format!("authentication failed for '{url}': credentials are only sent over https")
        }
        Reason::InvalidUrl => format!("'{url}' is not a valid Git URL"),
        Reason::UnusedParts(scheme) => {
            format!("the {scheme} protocol cannot use the user, password, host or port in '{url}'")
        }
        Reason::UnsupportedProtocol => format!("Toha cannot fetch from '{url}' over its protocol"),
        Reason::DeniedProtocol => {
            format!("the git configuration does not allow the protocol of '{url}'")
        }
        Reason::MissingReference(name) => {
            format!("the remote '{url}' has no reference named '{name}'")
        }
        Reason::AmbiguousReference(name) => format!(
            "the remote '{url}' has more than one reference named '{name}'; name one in full"
        ),
        Reason::MissingRevision(name) => format!("the remote '{url}' has no commit '{name}'"),
        Reason::Transport(detail) => format!("unable to access '{url}': {detail}"),
        Reason::Unknown(None) => match operation {
            Operation::Fetch => format!("could not fetch '{url}'"),
            Operation::List => format!("could not list the references of '{url}'"),
            Operation::Open => format!("could not open the repository '{url}'"),
            Operation::Save => format!("could not save the remote '{url}'"),
        },
        Reason::Unknown(Some(system)) => match operation {
            Operation::Fetch => format!("could not fetch '{url}': {system}"),
            Operation::List => format!("could not list the references of '{url}': {system}"),
            Operation::Open => format!("could not open the repository '{url}': {system}"),
            Operation::Save => format!("could not save the remote '{url}': {system}"),
        },
    }
}

/// Why an operation on a remote failed, as far as the error's types tell.
/// Every text a reason holds comes from Toha, curl's description of its error
/// code, the operating system's description of its error code, the URL's
/// scheme, or the name of a reference Toha asked for.
#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Authentication,
    CleartextCredentials,
    InvalidUrl,
    UnusedParts(String),
    UnsupportedProtocol,
    DeniedProtocol,
    MissingReference(String),
    AmbiguousReference(String),
    MissingRevision(String),
    Transport(String),
    Unknown(Option<String>),
}

fn reason(url: &str, error: &(dyn Error + 'static)) -> Reason {
    let http = url.starts_with("http://") || url.starts_with("https://");
    let mut system = None;
    let mut link = Some(error);
    while let Some(error) = link {
        if let Some(reason) = typed_reason(error) {
            return reason;
        }
        if let Some(curl) = curl_error(error) {
            return Reason::Transport(curl.description().to_owned());
        }
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            // The HTTP transport turns a 401 status into this kind.
            if http && io.kind() == ErrorKind::PermissionDenied {
                return Reason::Authentication;
            }
            if let Some(code) = io.raw_os_error() {
                system.get_or_insert(std::io::Error::from_raw_os_error(code).to_string());
            } else if http && io.kind() == ErrorKind::ConnectionAborted {
                // The HTTP transport turns a 5xx status into this kind.
                return Reason::Transport("the server reported an error".into());
            }
        }
        link = hidden(error).or_else(|| error.source());
    }
    Reason::Unknown(system)
}

/// The reason one gix error gives by its type and variant.
fn typed_reason(error: &(dyn Error + 'static)) -> Option<Reason> {
    use gix::protocol::{handshake, transport::client};
    if error.is::<gix::url::parse::Error>() {
        return Some(Reason::InvalidUrl);
    }
    // Asking the credential helpers or the resolver for credentials failed.
    if error.is::<gix::credentials::protocol::Error>() {
        return Some(Reason::Authentication);
    }
    if let Some(error) = error.downcast_ref::<handshake::Error>() {
        return match error {
            handshake::Error::Credentials(_)
            | handshake::Error::EmptyCredentials
            | handshake::Error::InvalidCredentials { .. } => Some(Reason::Authentication),
            _ => None,
        };
    }
    if let Some(client::Error::AuthenticationRefused(_)) = error.downcast_ref() {
        return Some(Reason::CleartextCredentials);
    }
    if let Some(error) = error.downcast_ref::<client::blocking_io::connect::Error>() {
        use client::blocking_io::connect::Error;
        return match error {
            Error::UnsupportedUrlTokens { scheme, .. } => {
                Some(Reason::UnusedParts(scheme.as_str().to_owned()))
            }
            Error::UnsupportedScheme(_) => Some(Reason::UnsupportedProtocol),
            _ => None,
        };
    }
    if let Some(error) = error.downcast_ref::<gix::remote::connect::Error>() {
        use gix::remote::connect::Error;
        return match error {
            Error::ProtocolDenied { .. } | Error::SchemePermission(_) => {
                Some(Reason::DeniedProtocol)
            }
            _ => None,
        };
    }
    if let Some(error) = error.downcast_ref::<gix::clone::fetch::Error>() {
        use gix::clone::fetch::Error;
        let name =
            |wanted: &gix::refs::PartialName| wanted.as_ref().as_bstr().to_str_lossy().into_owned();
        return match error {
            Error::RefNameMissing { wanted } => Some(Reason::MissingReference(name(wanted))),
            Error::RefNameAmbiguous { wanted, .. } => {
                Some(Reason::AmbiguousReference(name(wanted)))
            }
            Error::RevisionMissing { wanted } => {
                Some(Reason::MissingRevision(wanted.to_str_lossy().into_owned()))
            }
            _ => None,
        };
    }
    None
}

/// The error a transparent gix variant wraps. Such a variant shows the inner
/// error's text as its own and passes on the inner error's source, so
/// `source()` skips the inner error and the reason its type gives.
fn hidden<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a (dyn Error + 'static)> {
    use gix::protocol::{fetch, handshake, ls_refs, transport::client};
    if let Some(gix::clone::Error::UrlParse(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(error) = error.downcast_ref::<gix::clone::fetch::Error>() {
        use gix::clone::fetch::Error;
        return match error {
            Error::Connect(error) => Some(error),
            Error::PrepareFetch(error) => Some(error),
            Error::Fetch(error) => Some(error),
            Error::RemoteInit(error) => Some(error),
            Error::RefMap(error) => Some(error),
            _ => None,
        };
    }
    if let Some(gix::remote::fetch::prepare::Error::RefMap(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(gix::remote::init::Error::Url(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(gix::remote::connect::Error::Connect(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(client::blocking_io::connect::Error::Url(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(error) = error.downcast_ref::<gix::remote::fetch::Error>() {
        use gix::remote::fetch::Error;
        return match error {
            Error::Fetch(error) => Some(error),
            Error::Client(error) => Some(error),
            _ => None,
        };
    }
    if let Some(error) = error.downcast_ref::<gix::remote::ref_map::Error>() {
        use gix::remote::ref_map::Error;
        return match error {
            Error::InitRefMap(error) => Some(error),
            Error::Handshake(error) => Some(error),
            Error::Transport(error) => Some(error),
            _ => None,
        };
    }
    if let Some(fetch::Error::Client(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(fetch::refmap::init::Error::ListRefs(error)) = error.downcast_ref() {
        return Some(error);
    }
    if let Some(error) = error.downcast_ref::<ls_refs::Error>() {
        return match error {
            ls_refs::Error::Io(error) => Some(error),
            ls_refs::Error::Transport(error) => Some(error),
            _ => None,
        };
    }
    if let Some(handshake::Error::Transport(error)) = error.downcast_ref() {
        return Some(error);
    }
    None
}

/// The curl error `error` is, or wraps in an IO error.
fn curl_error<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a curl::Error> {
    error.downcast_ref::<curl::Error>().or_else(|| {
        error
            .downcast_ref::<std::io::Error>()?
            .get_ref()?
            .downcast_ref::<curl::Error>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://sample-user:sample-secret@example.invalid/owner/repo.git";
    const SAFE: &str = "https://example.invalid/owner/repo.git";

    /// An error whose text holds the URL with its password, as gix writes it,
    /// and whose source is `source`.
    #[derive(Debug)]
    struct Leaky(Option<Box<dyn Error + Send + Sync + 'static>>);

    impl std::fmt::Display for Leaky {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "failed for {URL:?} and https://sample-user:sample%2Dsecret@x"
            )
        }
    }

    impl Error for Leaky {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.0.as_deref().map(|e| e as &(dyn Error + 'static))
        }
    }

    fn leaky(source: impl Error + Send + Sync + 'static) -> Leaky {
        Leaky(Some(Box::new(source)))
    }

    #[track_caller]
    fn assert_message(error: &(dyn Error + 'static), operation: Operation, expected: &str) {
        let message = message(URL, operation, error);
        assert_eq!(message, expected);
        assert!(!message.contains("secret"), "{message}");
    }

    #[test]
    fn an_unknown_failure_names_the_url_without_credentials_and_nothing_else() {
        let error = Leaky(None);
        assert_message(
            &error,
            Operation::Fetch,
            &format!("could not fetch '{SAFE}'"),
        );
        assert_message(
            &error,
            Operation::List,
            &format!("could not list the references of '{SAFE}'"),
        );
        assert_message(
            &error,
            Operation::Open,
            &format!("could not open the repository '{SAFE}'"),
        );
        assert_message(
            &error,
            Operation::Save,
            &format!("could not save the remote '{SAFE}'"),
        );
    }

    #[test]
    fn a_rejected_login_is_an_authentication_failure() {
        let rejected = std::io::Error::new(ErrorKind::PermissionDenied, Leaky(None));
        let expected = format!("authentication failed for '{SAFE}'");
        assert_message(&leaky(rejected), Operation::Fetch, &expected);
        let unanswered = gix::credentials::protocol::Error::IdentityMissing {
            context: gix::credentials::protocol::Context::default(),
        };
        assert_message(&leaky(unanswered), Operation::List, &expected);
    }

    #[test]
    fn permission_denied_is_an_authentication_failure_only_over_http() {
        let denied = leaky(std::io::Error::from(ErrorKind::PermissionDenied));
        let message = message("file:///sample/repo.git", Operation::Fetch, &denied);
        assert_eq!(message, "could not fetch 'file:///sample/repo.git'");
    }

    #[test]
    fn a_transport_failure_gives_curls_description_of_its_code() {
        let mut error = curl::Error::new(7);
        error.set_extra(format!("Failed to connect to {URL}"));
        let error = std::io::Error::other(error);
        assert_message(
            &leaky(error),
            Operation::Fetch,
            &format!(
                "unable to access '{SAFE}': {}",
                curl::Error::new(7).description()
            ),
        );
    }

    #[test]
    fn a_server_error_is_named_without_its_text() {
        let error = std::io::Error::new(ErrorKind::ConnectionAborted, Leaky(None));
        assert_message(
            &leaky(error),
            Operation::Fetch,
            &format!("unable to access '{SAFE}': the server reported an error"),
        );
    }

    #[test]
    fn an_operating_system_error_gives_the_systems_description() {
        let error = std::io::Error::from_raw_os_error(2);
        let system = std::io::Error::from_raw_os_error(2).to_string();
        assert_message(
            &leaky(error),
            Operation::Open,
            &format!("could not open the repository '{SAFE}': {system}"),
        );
    }

    #[test]
    fn an_unparsable_url_is_named_invalid() {
        let error = gix::url::parse(gix::bstr::BStr::new("https://u:p%zz@host/r")).unwrap_err();
        assert_message(
            &leaky(error),
            Operation::Save,
            &format!("'{SAFE}' is not a valid Git URL"),
        );
    }

    #[test]
    fn credentials_a_protocol_cannot_use_are_named_without_them() {
        let url = gix::url::parse(gix::bstr::BStr::new("git://u:p%40ss@host/r")).unwrap();
        let error = gix::remote::connect::Error::Connect(
            gix::protocol::transport::client::blocking_io::connect::Error::UnsupportedUrlTokens {
                url: url.to_bstring(),
                scheme: url.scheme.clone(),
            },
        );
        let message = message("git://u:p@ss@host/r", Operation::Fetch, &error);
        assert_eq!(
            message,
            "the git protocol cannot use the user, password, host or port in 'git://host/r'"
        );
    }

    #[test]
    fn a_missing_reference_is_named() {
        let wanted = gix::refs::PartialName::try_from("v9").unwrap();
        let error = gix::clone::fetch::Error::RefNameMissing { wanted };
        assert_message(
            &error,
            Operation::Fetch,
            &format!("the remote '{SAFE}' has no reference named 'v9'"),
        );
    }
}
