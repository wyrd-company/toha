// ---
// relationships:
//   implements: architecture
// ---
//! Credentials a library caller supplies for HTTPS Git remotes, without a
//! credential helper process or a file on disk.
use gix::bstr::ByteSlice;
use gix::credentials::{helper, protocol};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex, PoisonError},
};

/// A username and password for an HTTPS Git remote. Its `Debug` output hides
/// the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    username: String,
    password: String,
}

impl Credential {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// What an HTTPS Git remote asks credentials for: its origin and the
/// repository path on it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CredentialRequest {
    /// Always `https`; only HTTPS remotes consult a resolver.
    pub scheme: String,
    /// The host, in lowercase.
    pub host: String,
    /// The port, with `443` when the address names none.
    pub port: u16,
    /// The repository path on the origin, percent-decoded, such as
    /// `/owner/repository.git`.
    pub path: String,
}

impl CredentialRequest {
    fn from_url(url: &gix::Url) -> Option<Self> {
        Some(Self {
            scheme: "https".into(),
            host: url.host()?.to_owned(),
            port: url.port_or_default()?,
            path: url.path.to_str_lossy().into_owned(),
        })
    }
}

/// Resolves credentials for the HTTPS Git remotes an operation connects to.
///
/// Toha asks only when an HTTPS remote requires authentication, once for each
/// challenge, with the origin and repository that challenged. After a redirect
/// the request names the origin the remote reached. `Ok(Some(_))` answers
/// that challenge; `Ok(None)` leaves it to the credentials of the Git
/// configuration, as without a resolver; `Err(reason)` fails the operation
/// with `reason`, which must not hold a secret.
pub trait CredentialResolver: Send + Sync {
    fn resolve(&self, request: &CredentialRequest) -> Result<Option<Credential>, String>;
}

impl<F> CredentialResolver for F
where
    F: Fn(&CredentialRequest) -> Result<Option<Credential>, String> + Send + Sync,
{
    fn resolve(&self, request: &CredentialRequest) -> Result<Option<Credential>, String> {
        self(request)
    }
}

thread_local! {
    static RESOLVER: RefCell<Option<Arc<dyn CredentialResolver>>> = const { RefCell::new(None) };
}

/// Run `operation` with `resolver` answering the HTTPS Git remotes it
/// connects to: template fetches, commit lookups and the `git` question's
/// remote probe.
///
/// The resolver applies on the calling thread until `operation` returns or
/// unwinds. Operations on other threads, and code after this call, keep the
/// behaviour without a resolver. An inner call replaces the resolver for its
/// own operation. SSH and local remotes never consult it.
///
/// While the resolver is in scope, curl's verbose output is off for the
/// operation's HTTP connections, and a clone does not fetch a remote branch
/// named `HEAD`, so that every connection consults the resolver.
pub fn with_credential_resolver<T>(
    resolver: Arc<dyn CredentialResolver>,
    operation: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Arc<dyn CredentialResolver>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            RESOLVER.with(|current| *current.borrow_mut() = previous);
        }
    }
    let _restore = Restore(RESOLVER.with(|current| current.replace(Some(resolver))));
    operation()
}

/// Whether the calling thread's operation has a resolver.
pub(crate) fn resolving() -> bool {
    RESOLVER.with(|current| current.borrow().is_some())
}

fn current_resolver() -> Option<Arc<dyn CredentialResolver>> {
    RESOLVER.with(|current| current.borrow().clone())
}

/// What the connections of one operation received from credential sources:
/// the passwords to keep out of its messages and the resolver's refusal.
#[derive(Clone, Default)]
pub(crate) struct Supplied(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    passwords: Vec<String>,
    refusal: Option<String>,
}

impl Supplied {
    /// Answer HTTPS challenges on `connection` from the resolver of the
    /// current operation. Other challenges keep the credentials of the Git
    /// configuration, and without a resolver the connection is unchanged.
    /// Returns whether a resolver is in scope.
    pub(crate) fn attach(&self, connection: &mut super::Connection<'_, '_, '_>) -> bool {
        let Some(resolver) = current_resolver() else {
            return false;
        };
        let fallback = connection.configured_credentials_for_current_url();
        connection.set_credentials(self.authenticate(resolver, fallback));
        true
    }

    /// The resolver's refusal, when it refused.
    pub(crate) fn refusal(&self) -> Option<String> {
        self.lock().refusal.clone()
    }

    // gix fixes the callback's error type.
    #[allow(clippy::result_large_err)]
    fn authenticate(
        &self,
        resolver: Arc<dyn CredentialResolver>,
        mut fallback: gix::remote::AuthenticateFn<'static>,
    ) -> impl FnMut(helper::Action) -> protocol::Result + use<> {
        let supplied = self.clone();
        // Whether the resolver answered the latest `Get`; its `Store` and
        // `Erase` follow-ups then concern no helper.
        let mut answered = false;
        move |action| {
            if let helper::Action::Get(context) = &action {
                answered = false;
                // The handshake names the transport's current URL, so a
                // redirected challenge names the origin it reached.
                let url = context
                    .url
                    .clone()
                    .or_else(|| context.to_url())
                    .ok_or(protocol::Error::UrlMissing)?;
                let request = gix::url::parse(url.as_bstr())
                    .ok()
                    .filter(|url| url.scheme == gix::url::Scheme::Https)
                    .and_then(|url| CredentialRequest::from_url(&url));
                if let Some(request) = request {
                    match resolver.resolve(&request) {
                        Ok(Some(credential)) => {
                            answered = true;
                            supplied.keep_out(&credential.password);
                            return Ok(Some(protocol::Outcome {
                                identity: gix::sec::identity::Account {
                                    username: credential.username,
                                    password: credential.password,
                                    oauth_refresh_token: None,
                                },
                                next: context.clone().into(),
                            }));
                        }
                        Ok(None) => {}
                        Err(reason) => {
                            supplied.lock().refusal = Some(reason);
                            return Err(protocol::Error::Quit);
                        }
                    }
                }
            } else if answered {
                return Ok(None);
            }
            let outcome = fallback(action)?;
            if let Some(outcome) = &outcome {
                supplied.keep_out(&outcome.identity.password);
            }
            Ok(outcome)
        }
    }

    /// The text to report for a failed operation: the resolver's refusal when
    /// it refused, otherwise `message`, with every supplied password removed.
    pub(crate) fn message(&self, message: String) -> String {
        let state = self.lock();
        let mut message = state.refusal.clone().unwrap_or(message);
        for password in state.passwords.iter().filter(|p| !p.is_empty()) {
            for form in written_forms(password) {
                message = message.replace(&form, "<redacted>");
            }
        }
        message
    }

    fn keep_out(&self, password: &str) {
        self.lock().passwords.push(password.to_owned());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The ways a gix message can write `password`: as text, as Rust escapes it
/// in a quoted string, and as the byte list gix shows for bytes a remote sent.
fn written_forms(password: &str) -> [String; 3] {
    let quoted = format!("{password:?}");
    let listed = format!("{:?}", password.as_bytes());
    [
        password.to_owned(),
        quoted[1..quoted.len() - 1].to_owned(),
        listed[1..listed.len() - 1].to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWERED: &str = "https://answered.invalid:8443/owner/r%20one.git";
    const DEFERRED: &str = "https://deferred.invalid/r";
    const REFUSED: &str = "https://refused.invalid/r";

    /// Answers `answered.invalid`, refuses `refused.invalid`, defers the rest,
    /// and records every request.
    #[derive(Default)]
    struct Sample(Mutex<Vec<CredentialRequest>>);

    impl CredentialResolver for Sample {
        fn resolve(&self, request: &CredentialRequest) -> Result<Option<Credential>, String> {
            self.0.lock().unwrap().push(request.clone());
            match request.host.as_str() {
                "answered.invalid" => Ok(Some(Credential::new("sample-user", "sample-password"))),
                "refused.invalid" => Err("the sample resolver refuses".into()),
                _ => Ok(None),
            }
        }
    }

    /// A callback under test with its resolver, what it remembers, and the
    /// actions its fallback received. The fallback answers every `Get` with
    /// `configured-password`.
    struct Harness<F> {
        supplied: Supplied,
        callback: F,
        resolver: Arc<Sample>,
        forwarded: Arc<Mutex<Vec<String>>>,
    }

    #[allow(clippy::result_large_err)]
    fn harness() -> Harness<impl FnMut(helper::Action) -> protocol::Result> {
        let resolver = Arc::new(Sample::default());
        let forwarded = Arc::new(Mutex::new(Vec::new()));
        let seen = forwarded.clone();
        let fallback: gix::remote::AuthenticateFn<'static> = Box::new(move |action| {
            seen.lock().unwrap().push(action.as_arg(true).to_owned());
            Ok(action.context().map(|context| protocol::Outcome {
                identity: gix::sec::identity::Account {
                    username: "configured-user".into(),
                    password: "configured-password".into(),
                    oauth_refresh_token: None,
                },
                next: context.clone().into(),
            }))
        });
        let supplied = Supplied::default();
        let callback = supplied.authenticate(resolver.clone(), fallback);
        Harness {
            supplied,
            callback,
            resolver,
            forwarded,
        }
    }

    impl<F> Harness<F> {
        fn hosts(&self) -> Vec<String> {
            let asked = self.resolver.0.lock().unwrap();
            asked.iter().map(|r| r.host.clone()).collect()
        }

        fn forwarded(&self) -> Vec<String> {
            self.forwarded.lock().unwrap().clone()
        }
    }

    fn password(outcome: protocol::Result) -> String {
        outcome.unwrap().unwrap().identity.password
    }

    #[test]
    fn an_https_challenge_takes_the_resolver_credential() {
        let mut h = harness();
        let get = helper::Action::get_for_url(ANSWERED.replace("://", "://user:old@"));
        assert_eq!(password((h.callback)(get)), "sample-password");
        assert_eq!(
            *h.resolver.0.lock().unwrap(),
            [CredentialRequest {
                scheme: "https".into(),
                host: "answered.invalid".into(),
                port: 8443,
                path: "/owner/r one.git".into(),
            }]
        );
        assert!(h.forwarded().is_empty());
        let message = h.supplied.message("echo sample-password".into());
        assert_eq!(message, "echo <redacted>");
    }

    #[test]
    fn a_request_names_the_default_port_and_a_lowercase_host() {
        let mut h = harness();
        (h.callback)(helper::Action::get_for_url("https://Deferred.Invalid/r")).unwrap();
        let asked = h.resolver.0.lock().unwrap();
        assert_eq!(
            (asked[0].host.as_str(), asked[0].port),
            ("deferred.invalid", 443)
        );
    }

    #[test]
    fn other_schemes_keep_the_configured_credentials() {
        let mut h = harness();
        for url in ["http://answered.invalid/r", "ssh://answered.invalid/r"] {
            let get = helper::Action::get_for_url(url);
            assert_eq!(password((h.callback)(get)), "configured-password");
        }
        assert!(h.hosts().is_empty());
        assert_eq!(h.forwarded(), ["get", "get"]);
    }

    #[test]
    fn a_deferral_keeps_the_configured_credentials() {
        let mut h = harness();
        (h.callback)(helper::Action::get_for_url(ANSWERED)).unwrap();
        let get = helper::Action::get_for_url(DEFERRED);
        assert_eq!(password((h.callback)(get)), "configured-password");
        assert_eq!(h.hosts(), ["answered.invalid", "deferred.invalid"]);
        (h.callback)(helper::Action::Erase("previous".into())).unwrap();
        assert_eq!(h.forwarded(), ["get", "erase"]);
        let message = h.supplied.message("echo configured-password".into());
        assert_eq!(message, "echo <redacted>");
    }

    #[test]
    fn a_refusal_fails_with_its_reason() {
        let mut h = harness();
        let get = helper::Action::get_for_url(REFUSED);
        assert!(matches!((h.callback)(get), Err(protocol::Error::Quit)));
        assert!(h.forwarded().is_empty());
        let message = h.supplied.message("The handler asked to stop".into());
        assert_eq!(message, "the sample resolver refuses");
    }

    #[test]
    fn follow_ups_of_a_resolver_answer_reach_no_helper() {
        let mut h = harness();
        (h.callback)(helper::Action::get_for_url(DEFERRED)).unwrap();
        (h.callback)(helper::Action::get_for_url(ANSWERED)).unwrap();
        (h.callback)(helper::Action::Store("previous".into())).unwrap();
        (h.callback)(helper::Action::Erase("previous".into())).unwrap();
        assert_eq!(h.forwarded(), ["get"]);
    }

    fn is_current(resolver: &Arc<dyn CredentialResolver>) -> bool {
        current_resolver().is_some_and(|current| Arc::ptr_eq(&current, resolver))
    }

    #[test]
    fn a_resolver_applies_only_inside_its_operation() {
        let outer: Arc<dyn CredentialResolver> = Arc::new(Sample::default());
        let inner: Arc<dyn CredentialResolver> = Arc::new(Sample::default());
        assert!(current_resolver().is_none());
        with_credential_resolver(outer.clone(), || {
            assert!(is_current(&outer));
            with_credential_resolver(inner.clone(), || assert!(is_current(&inner)));
            assert!(is_current(&outer));
            // Another thread keeps the behaviour without a resolver.
            std::thread::spawn(|| assert!(current_resolver().is_none()))
                .join()
                .unwrap();
        });
        assert!(current_resolver().is_none());
    }

    #[test]
    fn an_unwinding_operation_restores_the_previous_resolver() {
        let resolver: Arc<dyn CredentialResolver> = Arc::new(Sample::default());
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_credential_resolver(resolver.clone(), || panic!("sample operation fails"))
        }));
        assert!(unwound.is_err());
        assert!(current_resolver().is_none());
    }

    #[test]
    fn debug_hides_the_password() {
        let shown = format!("{:?}", Credential::new("sample-user", "sample-password"));
        assert!(shown.contains("sample-user"), "{shown}");
        assert!(!shown.contains("sample-password"), "{shown}");
    }

    #[test]
    fn messages_hold_no_written_form_of_a_password() {
        let supplied = Supplied::default();
        supplied.keep_out("sample\"password");
        let message = supplied.message(format!(
            "raw sample\"password, quoted {:?}, listed {:?}",
            "sample\"password", b"sample\"password"
        ));
        assert_eq!(
            message,
            "raw <redacted>, quoted \"<redacted>\", listed [<redacted>]"
        );
    }

    #[test]
    fn an_empty_password_leaves_the_message_intact() {
        let supplied = Supplied::default();
        supplied.keep_out("");
        assert_eq!(supplied.message("unchanged".into()), "unchanged");
    }
}
