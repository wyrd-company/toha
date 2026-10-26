// ---
// relationships:
//   implements: architecture
// ---
//! A local smart-HTTP Git endpoint over TLS exercises the real HTTPS transport.
#[allow(dead_code)]
mod support;
use base64::Engine;
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use tempfile::TempDir;
use toha::source::{Address, RefKind, fetch};

fn checked(mut command: Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn git(cwd: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1");
    checked(cmd);
}
fn serve(
    mut stream: StreamOwned<ServerConnection, std::net::TcpStream>,
    root: &Path,
    auth: Option<&Auth>,
) {
    if serve_http(&mut stream, root, auth) {
        // rustls rejects TCP EOF without the TLS close_notify alert, even after a
        // complete HTTP response. OpenSSL can accept that truncated shutdown.
        stream.conn.send_close_notify();
        stream.flush().unwrap();
    }
}

/// The basic credential an endpoint requires. A misbehaving endpoint echoes a
/// rejected credential back as its service announcement, which gix reports.
#[derive(Clone)]
struct Auth {
    username: &'static str,
    password: &'static str,
    echo: bool,
    /// The origin an authorized request is redirected to, as `https://host:port`.
    redirect: Option<String>,
    /// The origin every request is redirected to before any challenge.
    moved: Option<String>,
    /// The decoded basic credential of every request, empty for none.
    received: Arc<Mutex<Vec<String>>>,
}

impl Auth {
    fn new(username: &'static str, password: &'static str) -> Self {
        Self {
            username,
            password,
            echo: false,
            redirect: None,
            moved: None,
            received: Arc::default(),
        }
    }

    fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

fn serve_http(stream: &mut (impl Read + Write), root: &Path, auth: Option<&Auth>) -> bool {
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 8192];
        let size = match stream.read(&mut chunk) {
            Ok(size) => size,
            Err(_) => return false, // An untrusted TLS client closes before HTTP.
        };
        if size == 0 {
            return false;
        }
        request.extend_from_slice(&chunk[..size]);
        if let Some(pos) = request.windows(4).position(|v| v == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let mut lines = headers.lines();
    let first = lines.next().unwrap();
    let mut words = first.split_whitespace();
    let method = words.next().unwrap();
    let target = words.next().unwrap();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut content_length = 0;
    let mut content_type = String::new();
    let mut authorization = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            authorization = value
                .trim()
                .strip_prefix("Basic ")
                .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
                .map(|v| String::from_utf8_lossy(&v).into_owned());
        }
        if let Some(value) = line
            .strip_prefix("Content-Length: ")
            .or_else(|| line.strip_prefix("content-length: "))
        {
            content_length = value.trim().parse::<usize>().unwrap();
        }
        if let Some(value) = line
            .strip_prefix("Content-Type: ")
            .or_else(|| line.strip_prefix("content-type: "))
        {
            content_type = value.trim().into();
        }
    }
    while request.len() - header_end < content_length {
        let mut chunk = [0; 8192];
        let size = stream.read(&mut chunk).unwrap();
        if size == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..size]);
    }
    if let Some(auth) = auth {
        let expected = format!("{}:{}", auth.username, auth.password);
        let given = authorization.clone().unwrap_or_default();
        auth.received.lock().unwrap().push(given);
        if let Some(origin) = &auth.moved {
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {origin}{target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
            return true;
        }
        match authorization {
            Some(given) if given == expected => {
                if let Some(origin) = &auth.redirect {
                    let response = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {origin}{target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    stream.flush().unwrap();
                    return true;
                }
            }
            Some(given) if auth.echo && !given.ends_with(':') => {
                let line = format!("# service={given}\n");
                let body = format!("{:04x}{line}0000", line.len() + 4);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
                return true;
            }
            _ => {
                let response = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"fixture\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
                return true;
            }
        }
    }
    let mut child = Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("REQUEST_METHOD", method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", content_length.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&request[header_end..header_end + content_length])
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let separator = output
        .stdout
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .map(|p| (p, 4))
        .or_else(|| {
            output
                .stdout
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|p| (p, 2))
        })
        .unwrap();
    let (head, body) = output.stdout.split_at(separator.0 + separator.1);
    let head = String::from_utf8_lossy(head);
    let status = head
        .lines()
        .find_map(|line| line.strip_prefix("Status: "))
        .unwrap_or("200 OK")
        .trim();
    let header_lines: String = head
        .lines()
        .filter(|line| !line.starts_with("Status:") && !line.trim().is_empty())
        .map(|line| format!("{}\r\n", line.trim_end_matches('\r')))
        .collect();
    let response = format!(
        "HTTP/1.1 {status}\r\n{header_lines}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
    true
}
#[test]
fn fetches_https_git_repository() {
    run_https_fixture(Trust::Explicit);
}

#[test]
fn fetches_https_git_repository_with_platform_trust() {
    if !bundled_rustls() {
        return; // System curl already supplies its platform verifier.
    }
    run_https_fixture(Trust::Platform);
}

#[test]
fn rejects_https_git_repository_with_untrusted_platform_certificate() {
    if !bundled_rustls() {
        return;
    }
    run_https_fixture(Trust::Untrusted);
}

#[test]
fn explicit_git_ca_overrides_platform_trust() {
    run_https_fixture(Trust::GitConfig);
}

#[test]
fn fetches_http_git_repository_without_ca_certificates() {
    run_https_fixture(Trust::Http);
}

#[test]
fn rejects_https_git_repository_without_ca_certificates() {
    run_https_fixture(Trust::NoRoots);
}

#[test]
fn preserves_explicit_git_ssl_verification_setting() {
    run_https_fixture(Trust::VerificationDisabled);
}

fn bundled_rustls() -> bool {
    curl::Version::get()
        .ssl_version()
        .is_some_and(|backend| backend.starts_with("rustls"))
}

#[derive(Clone, Copy)]
enum Trust {
    Explicit,
    Platform,
    Untrusted,
    GitConfig,
    Http,
    NoRoots,
    VerificationDisabled,
}

fn configure_trust(command: &mut Command, root: &Path, trust: Trust) {
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join("fixture.gitconfig"))
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_SSL_NO_VERIFY")
        .env_remove("GIT_SSL_CAINFO")
        .env_remove("CURL_CA_BUNDLE")
        .env("SSL_CERT_DIR", root.join("empty-certs"))
        .env("NO_PROXY", "localhost,127.0.0.1")
        .env("no_proxy", "localhost,127.0.0.1");
    let cert = root.join("cert.pem");
    let untrusted = root.join("untrusted.pem");
    match trust {
        Trust::Explicit => {
            command
                .env("GIT_SSL_CAINFO", cert)
                .env("SSL_CERT_FILE", untrusted);
        }
        Trust::Platform => {
            command.env("SSL_CERT_FILE", cert);
        }
        Trust::Untrusted => {
            command.env("SSL_CERT_FILE", untrusted);
        }
        Trust::GitConfig => {
            command.env("SSL_CERT_FILE", untrusted);
        }
        Trust::Http | Trust::NoRoots => {
            command.env("SSL_CERT_FILE", root.join("empty.pem"));
        }
        Trust::VerificationDisabled => {
            command
                .env("SSL_CERT_FILE", root.join("empty.pem"))
                .env("GIT_SSL_NO_VERIFY", "1");
        }
    }
}

/// A template repository at `remote.git` under a new root, with the fixture
/// certificates, and the TLS configuration that serves them.
fn https_fixture(trust: Trust) -> (TempDir, ServerConfig) {
    let root = TempDir::new().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-b", "main"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    git(&source, &["config", "user.name", "Test"]);
    fs::write(source.join("template.yml"), "name: sample\ninterview: []\n").unwrap();
    fs::create_dir(source.join("template")).unwrap();
    fs::write(source.join("template/.gitkeep"), "").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(
        root.path(),
        &["clone", "--bare", source.to_str().unwrap(), "remote.git"],
    );
    let cert = root.path().join("cert.pem");
    let key = root.path().join("key.pem");
    let mut openssl = Command::new("openssl");
    openssl.args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        key.to_str().unwrap(),
        "-out",
        cert.to_str().unwrap(),
        "-days",
        "1",
        "-subj",
        "/CN=localhost",
        "-addext",
        "subjectAltName=DNS:localhost",
        "-addext",
        "basicConstraints=critical,CA:FALSE",
    ]);
    checked(openssl);
    let mut unrelated = Command::new("openssl");
    unrelated.args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "1",
        "-subj",
        "/CN=unrelated",
        "-keyout",
        root.path().join("untrusted.key").to_str().unwrap(),
        "-out",
        root.path().join("untrusted.pem").to_str().unwrap(),
    ]);
    checked(unrelated);
    fs::create_dir(root.path().join("empty-certs")).unwrap();
    fs::write(root.path().join("empty.pem"), "").unwrap();
    let git_config = if matches!(trust, Trust::GitConfig) {
        format!(
            "[http]\nsslCAInfo = {}\n",
            cert.to_string_lossy().replace('\\', "/")
        )
    } else {
        String::new()
    };
    fs::write(root.path().join("fixture.gitconfig"), git_config).unwrap();
    let mut convert = Command::new("openssl");
    convert.args(["x509", "-in", cert.to_str().unwrap(), "-outform", "DER"]);
    let cert_der = checked(convert);
    let mut convert = Command::new("openssl");
    convert.args([
        "pkcs8",
        "-topk8",
        "-nocrypt",
        "-in",
        key.to_str().unwrap(),
        "-outform",
        "DER",
    ]);
    let key_der = checked(convert);
    // OpenSSL accepts the old CA-as-server fixture, but rustls rejects it with
    // CaUsedAsEndEntity. Validate the certificate with the stricter verifier.
    let mut roots = rustls::RootCertStore::empty();
    let certificate = CertificateDer::from(cert_der.clone());
    roots.add(certificate.clone()).unwrap();
    let verifier = rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .unwrap();
    rustls::client::danger::ServerCertVerifier::verify_server_cert(
        &*verifier,
        &certificate,
        &[],
        &rustls::pki_types::ServerName::try_from("localhost").unwrap(),
        &[],
        rustls::pki_types::UnixTime::now(),
    )
    .expect("the fixture certificate must be valid for rustls as well as OpenSSL");
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert_der)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .unwrap();
    (root, config)
}

/// A local smart-HTTP endpoint for `root`, over TLS when `tls` is given.
struct Endpoint {
    port: u16,
    stop: Arc<AtomicBool>,
    server: thread::JoinHandle<()>,
}

impl Endpoint {
    fn start(root: &Path, tls: Option<ServerConfig>, auth: Option<Auth>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let root_path = root.to_path_buf();
        let server = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        // Accepted sockets inherit nonblocking mode on some platforms.
                        socket.set_nonblocking(false).unwrap();
                        if let Some(config) = &tls {
                            let conn = ServerConnection::new(Arc::new(config.clone())).unwrap();
                            serve(StreamOwned::new(conn, socket), &root_path, auth.as_ref());
                        } else {
                            let mut socket = socket;
                            serve_http(&mut socket, &root_path, auth.as_ref());
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self { port, stop, server }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.join().unwrap();
    }
}

fn run_https_fixture(trust: Trust) {
    let (root, config) = https_fixture(trust);
    let tls = (!matches!(trust, Trust::Http)).then_some(config);
    let endpoint = Endpoint::start(root.path(), tls, None);
    let port = endpoint.port;
    // Run the crate API in a child so all transport configuration is scoped to
    // that process, with the same isolation as the command drivers.
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root.path());
    configure_trust(&mut worker, root.path(), trust);
    let scheme = if matches!(trust, Trust::Http) {
        "http"
    } else {
        "https"
    };
    let fetched = worker
        .args(["--ignored", "--exact", "https_fetch_worker", "--nocapture"])
        .env("TOHA_HTTPS_FIXTURE_ROOT", root.path())
        .env(
            "TOHA_HTTPS_EXPECT_SUCCESS",
            if matches!(trust, Trust::Untrusted | Trust::NoRoots) {
                "false"
            } else {
                "true"
            },
        )
        .env(
            "TOHA_HTTPS_FIXTURE_URL",
            format!("{scheme}://localhost:{port}/remote.git"),
        )
        .output()
        .unwrap();
    let token = "sample-secret";
    let url = if matches!(trust, Trust::Http) {
        format!("http://localhost:{port}/remote.git")
    } else {
        format!("https://user:{token}@localhost:{port}/remote.git")
    };
    let mut command = support::isolated_command(root.path());
    configure_trust(&mut command, root.path(), trust);
    let add = command
        .args(["templates", "add", &url])
        .current_dir(root.path())
        .output()
        .unwrap();

    let mut command = support::isolated_command(root.path());
    configure_trust(&mut command, root.path(), trust);
    let failed = command
        .args(["templates", "add", &format!("{url}@absent")])
        .current_dir(root.path())
        .output()
        .unwrap();
    endpoint.stop();
    assert!(
        fetched.status.success(),
        "{}{}",
        String::from_utf8_lossy(&fetched.stdout),
        String::from_utf8_lossy(&fetched.stderr)
    );
    if matches!(trust, Trust::Untrusted | Trust::NoRoots) {
        assert!(
            !add.status.success(),
            "an untrusted server certificate must fail"
        );
        assert!(!root.path().join("fetched/template.yml").exists());
        return;
    }
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let registry =
        fs::read_to_string(support::user_data_dir(root.path()).join("templates.yml")).unwrap();
    assert!(!registry.contains(token));
    let installs = fs::read_dir(support::user_data_dir(root.path()).join("repos")).unwrap();
    for install in installs {
        assert!(
            !install
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(token)
        );
    }
    assert!(!failed.status.success());
    assert!(!String::from_utf8_lossy(&failed.stderr).contains(token));
    assert!(root.path().join("fetched/template.yml").is_file());
}

#[test]
#[ignore = "subprocess helper for fetches_https_git_repository"]
fn https_fetch_worker() {
    let Some(root) = std::env::var_os("TOHA_HTTPS_FIXTURE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let address = Address::Git {
        repo: std::env::var("TOHA_HTTPS_FIXTURE_URL").unwrap(),
        reference: None,
        path: None,
    };
    let result = fetch(&address, &root.join("fetched"));
    if std::env::var("TOHA_HTTPS_EXPECT_SUCCESS").as_deref() == Ok("false") {
        assert!(result.is_err(), "an untrusted server certificate must fail");
        return;
    }
    let fetched = result.unwrap();
    assert_eq!(fetched.reference_kind, RefKind::DefaultBranch);
    assert_eq!(fetched.commit.len(), 40);
    assert!(root.join("fetched/template.yml").is_file());
}

fn private() -> Auth {
    Auth::new("fixture-user", "fixture-password")
}

fn other() -> Auth {
    Auth::new("other-user", "other-password")
}

/// The switches that turn on curl and Git transport diagnostics.
const DIAGNOSTICS: [&str; 6] = [
    "GIT_CURL_VERBOSE",
    "GIT_TRACE",
    "GIT_TRACE_CURL",
    "GIT_TRACE_PACKET",
    "GIT_TRACE2",
    "GIT_TRACE2_EVENT",
];

/// Run `credential_worker` with `scenario` against `urls`. The worker is its
/// own process, so its transport configuration stays out of this one.
fn run_credential_worker(root: &Path, trust: Trust, scenario: &str, urls: &[String]) {
    run_credential_worker_with(root, trust, scenario, urls, false);
}

/// [`run_credential_worker`], with every diagnostic switch on when
/// `diagnostics`. Returns what the worker wrote.
fn run_credential_worker_with(
    root: &Path,
    trust: Trust,
    scenario: &str,
    urls: &[String],
    diagnostics: bool,
) -> String {
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root);
    configure_trust(&mut worker, root, trust);
    for switch in DIAGNOSTICS {
        if diagnostics {
            worker.env(switch, "1");
        } else {
            worker.env_remove(switch);
        }
    }
    let output = worker
        .args(["--ignored", "--exact", "credential_worker", "--nocapture"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .stdin(Stdio::null())
        .env("TOHA_CREDENTIAL_SCENARIO", scenario)
        .env("TOHA_HTTPS_FIXTURE_ROOT", root)
        .env("TOHA_CREDENTIAL_URLS", urls.join(" "))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}{stderr}"
    );
    format!("{stdout}{stderr}")
}

fn url(endpoint: &Endpoint, scheme: &str) -> String {
    format!("{scheme}://localhost:{}/remote.git", endpoint.port)
}

/// Serve one private repository over TLS and run `scenario` against it.
/// Returns the credentials the endpoint received.
fn run_credential_scenario(scenario: &str, auth: Auth) -> Vec<String> {
    let (root, config) = https_fixture(Trust::Explicit);
    let endpoint = Endpoint::start(root.path(), Some(config), Some(auth.clone()));
    let urls = [url(&endpoint, "https")];
    run_credential_worker(root.path(), Trust::Explicit, scenario, &urls);
    endpoint.stop();
    auth.received()
}

#[test]
fn a_credential_resolver_fetches_a_private_https_repository() {
    run_credential_scenario("answers", private());
}

#[test]
fn a_declining_credential_resolver_leaves_git_configuration_in_charge() {
    run_credential_scenario("declines", private());
}

#[test]
fn a_refusing_credential_resolver_fails_with_its_reason() {
    run_credential_scenario("refuses", private());
}

#[test]
fn a_credential_resolver_refuses_another_host_or_repository() {
    let received = run_credential_scenario("scoped", private());
    assert!(
        !received
            .iter()
            .any(|given| given.contains("fixture-password")),
        "{received:?}"
    );
}

#[test]
fn a_rejected_resolver_password_stays_out_of_errors() {
    run_credential_scenario("rejected", private());
}

#[test]
fn an_echoed_resolver_password_stays_out_of_errors() {
    run_credential_scenario(
        "echoed",
        Auth {
            echo: true,
            ..private()
        },
    );
}

#[test]
fn a_credential_resolver_is_not_asked_for_plain_http() {
    let (root, _) = https_fixture(Trust::Http);
    let endpoint = Endpoint::start(root.path(), None, Some(private()));
    run_credential_worker(root.path(), Trust::Http, "plain", &[url(&endpoint, "http")]);
    endpoint.stop();
}

/// Serve the repository at a private second origin and a first origin that
/// redirects to it, `before` or after its own challenge, and run `scenario`.
/// Returns the credentials each origin received.
fn run_redirect_scenario(scenario: &str, before: bool) -> (Vec<String>, Vec<String>) {
    let (root, config) = https_fixture(Trust::Explicit);
    let target = other();
    let second = Endpoint::start(root.path(), Some(config.clone()), Some(target.clone()));
    let origin = Some(format!("https://localhost:{}", second.port));
    let source = if before {
        Auth {
            moved: origin,
            ..private()
        }
    } else {
        Auth {
            redirect: origin,
            ..private()
        }
    };
    let first = Endpoint::start(root.path(), Some(config), Some(source.clone()));
    let urls = [url(&first, "https"), url(&second, "https")];
    run_credential_worker(root.path(), Trust::Explicit, scenario, &urls);
    first.stop();
    second.stop();
    (source.received(), target.received())
}

#[test]
fn a_redirect_to_another_origin_asks_the_resolver_for_that_origin() {
    let (first, second) = run_redirect_scenario("moved", true);
    assert!(
        first.iter().all(|given| !given.contains("password")),
        "{first:?}"
    );
    assert!(
        second.contains(&"other-user:other-password".to_owned()),
        "{second:?}"
    );
    assert!(
        !second
            .iter()
            .any(|given| given.contains("fixture-password")),
        "{second:?}"
    );
}

#[test]
fn a_credential_never_follows_a_redirect_to_another_origin() {
    let (first, second) = run_redirect_scenario("redirected", false);
    assert!(
        first.contains(&"fixture-user:fixture-password".to_owned()),
        "{first:?}"
    );
    assert!(
        second.iter().all(|given| !given.contains("password")),
        "{second:?}"
    );
}

#[test]
fn credential_resolvers_of_concurrent_operations_stay_apart() {
    let (root, config) = https_fixture(Trust::Explicit);
    let first = Endpoint::start(root.path(), Some(config.clone()), Some(private()));
    let second = Endpoint::start(root.path(), Some(config), Some(other()));
    let urls = [url(&first, "https"), url(&second, "https")];
    run_credential_worker(root.path(), Trust::Explicit, "concurrent", &urls);
    first.stop();
    second.stop();
}

#[test]
fn transport_diagnostics_never_show_a_resolver_credential() {
    let (root, config) = https_fixture(Trust::Explicit);
    let auth = private();
    let endpoint = Endpoint::start(root.path(), Some(config), Some(auth.clone()));
    let urls = [url(&endpoint, "https")];
    let written = run_credential_worker_with(root.path(), Trust::Explicit, "answers", &urls, true);
    endpoint.stop();
    // The endpoint did take the credential, so each request carried it.
    assert!(
        auth.received()
            .contains(&"fixture-user:fixture-password".to_owned())
    );
    let basic = base64::engine::general_purpose::STANDARD.encode("fixture-user:fixture-password");
    let listed = format!("{:?}", "fixture-password".as_bytes());
    for form in [
        "fixture-password",
        basic.as_str(),
        format!("{:?}", "fixture-password").trim_matches('"'),
        listed.trim_start_matches('[').trim_end_matches(']'),
    ] {
        assert!(!written.contains(form), "{form} in {written}");
    }
}

#[test]
fn a_remote_head_branch_keeps_the_clone_on_the_resolver() {
    let (root, config) = https_fixture(Trust::Explicit);
    let source = root.path().join("source");
    let first = String::from_utf8(checked({
        let mut command = Command::new("git");
        command.args(["rev-parse", "HEAD"]).current_dir(&source);
        command
    }))
    .unwrap();
    git(&source, &["commit", "--allow-empty", "-m", "second"]);
    git(&source, &["tag", "sample-tag"]);
    // A tag no branch reaches arrives only when the clone asks for all tags.
    git(&source, &["checkout", "-q", "-b", "side"]);
    git(&source, &["commit", "--allow-empty", "-m", "side"]);
    git(&source, &["tag", "side-tag"]);
    git(&source, &["checkout", "-q", "main"]);
    git(
        &source,
        &["push", "../remote.git", "main", "sample-tag", "side-tag"],
    );
    // A branch named `HEAD` clashes with the clone's own `HEAD` tracking ref.
    let remote = root.path().join("remote.git");
    git(&remote, &["update-ref", "refs/heads/HEAD", first.trim()]);
    let endpoint = Endpoint::start(root.path(), Some(config), Some(private()));
    run_credential_worker(
        root.path(),
        Trust::Explicit,
        "head-branch",
        &[url(&endpoint, "https")],
    );
    endpoint.stop();
}

#[test]
fn an_apply_stops_unchanged_when_the_resolver_refuses_its_remote() {
    run_credential_scenario("apply-refused", private());
}

#[test]
#[ignore = "subprocess helper for the credential resolver scenarios"]
fn credential_worker() {
    use toha::{
        git_target::{RemoteState, probe},
        source::{
            Credential, CredentialRequest, CredentialResolver, SourceError,
            with_credential_resolver,
        },
    };
    let Ok(scenario) = std::env::var("TOHA_CREDENTIAL_SCENARIO") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("TOHA_HTTPS_FIXTURE_ROOT").unwrap());
    let urls: Vec<String> = std::env::var("TOHA_CREDENTIAL_URLS")
        .unwrap()
        .split(' ')
        .map(str::to_owned)
        .collect();
    // The address names a user, which the resolver must not receive.
    let with_user = |url: &str| url.replacen("://", "://address-user@", 1);
    let port = |url: &str| -> u16 {
        let authority = url.split('/').nth(2).unwrap();
        authority.rsplit(':').next().unwrap().parse().unwrap()
    };
    let attempt = std::sync::atomic::AtomicUsize::new(0);
    let fetch_from = |url: &str| -> Result<toha::source::Fetched, SourceError> {
        let n = attempt.fetch_add(1, Ordering::Relaxed);
        let address = Address::Git {
            repo: with_user(url),
            reference: None,
            path: None,
        };
        fetch(&address, &root.join(format!("fetched-{n}")))
    };
    /// Grants `credential` to the challenges it accepts and records each one.
    struct Recorder<F> {
        asked: Mutex<Vec<CredentialRequest>>,
        answer: F,
    }
    impl<F> CredentialResolver for Recorder<F>
    where
        F: Fn(&CredentialRequest) -> Result<Option<Credential>, String> + Send + Sync,
    {
        fn resolve(&self, request: &CredentialRequest) -> Result<Option<Credential>, String> {
            self.asked.lock().unwrap().push(request.clone());
            (self.answer)(request)
        }
    }
    fn recorder<F>(answer: F) -> Arc<Recorder<F>>
    where
        F: Fn(&CredentialRequest) -> Result<Option<Credential>, String> + Send + Sync,
    {
        Arc::new(Recorder {
            asked: Mutex::default(),
            answer,
        })
    }
    let grant = |auth: Auth, expected_port: u16| {
        recorder(move |request: &CredentialRequest| {
            if request.host != "localhost" || request.port != expected_port {
                return Err("these credentials belong to another origin".into());
            }
            if request.path != "/remote.git" {
                return Err("these credentials belong to another repository".into());
            }
            Ok(Some(Credential::new(auth.username, auth.password)))
        })
    };
    let main = urls[0].clone();
    let rejected = "rejected-sample-value";
    match scenario.as_str() {
        "answers" => {
            let resolver = grant(private(), port(&main));
            with_credential_resolver(resolver.clone(), || {
                let fetched = fetch_from(&main).unwrap();
                assert_eq!(fetched.commit.len(), 40);
                assert_eq!(probe(&with_user(&main)), RemoteState::HasHistory);
            });
            let asked = resolver.asked.lock().unwrap().clone();
            assert!(!asked.is_empty());
            for request in &asked {
                assert_eq!(request.scheme, "https");
                assert_eq!(request.host, "localhost");
                assert_eq!(request.port, port(&main));
                assert_eq!(request.path, "/remote.git");
            }
            // After the operation the default applies again.
            assert!(fetch_from(&main).is_err());
            assert_eq!(resolver.asked.lock().unwrap().len(), asked.len());
        }
        "head-branch" => {
            let remote = root.join("remote.git");
            let main_commit = String::from_utf8(checked({
                let mut command = Command::new("git");
                command
                    .args(["--git-dir", remote.to_str().unwrap()])
                    .args(["rev-parse", "refs/heads/main"]);
                command
            }))
            .unwrap();
            let resolver = grant(private(), port(&main));
            let (fetched, tagged) = with_credential_resolver(resolver.clone(), || {
                let tagged = Address::Git {
                    repo: with_user(&main),
                    reference: Some("sample-tag".into()),
                    path: None,
                };
                (
                    fetch_from(&main).unwrap(),
                    fetch(&tagged, &root.join("tagged")).unwrap(),
                )
            });
            assert_eq!(fetched.commit, main_commit.trim());
            let tags = String::from_utf8(checked({
                let mut command = Command::new("git");
                command
                    .args(["tag", "--list"])
                    .current_dir(root.join("fetched-0"));
                command
            }))
            .unwrap();
            assert!(tags.lines().any(|tag| tag == "side-tag"), "{tags}");
            assert_eq!(fetched.reference_kind, RefKind::DefaultBranch);
            assert_eq!(tagged.reference_kind, RefKind::Tag);
            // One challenge on each fetch's only connection.
            assert_eq!(resolver.asked.lock().unwrap().len(), 2);
        }
        "apply-refused" => {
            let template_root = root.join("git-template");
            fs::create_dir_all(template_root.join("template")).unwrap();
            fs::write(
                template_root.join("template.yml"),
                "name: sample\ninterview:\n  - id: remote\n    type: git\n    prompt: Remote\n",
            )
            .unwrap();
            fs::write(template_root.join("template/README.md"), "sample\n").unwrap();
            let template = toha::Template::load(&template_root).unwrap();
            fs::create_dir(root.join("target")).unwrap();
            let target = toha::staging::canonical_target(&root.join("target")).unwrap();
            let seed = toha::Seed {
                now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                defaults: Default::default(),
                context: toha::context::InvocationContext::for_target(target.clone()),
            };
            let toha::Interview::Asking(pending) = toha::Interview::start(&template, seed).unwrap()
            else {
                panic!("the git question asks");
            };
            let answers = [(
                toha::Id::parse("remote").unwrap(),
                toha::RawAnswer(serde_json::json!(main)),
            )]
            .into_iter()
            .collect();
            let toha::Interview::Complete(completed) = pending.answer(answers).unwrap() else {
                panic!("the interview completes");
            };
            let plan = toha::Plan::build(&template, &completed, &target).unwrap();
            struct NoHooks;
            impl toha::hook::HookRunner for NoHooks {
                fn run(
                    &self,
                    _: &toha::PlannedHook,
                    _: &Path,
                ) -> Result<toha::hook::HookOutcome, toha::hook::HookError> {
                    panic!("the template has no hooks")
                }
            }
            let reason = "scope denied";
            let resolver = recorder(move |_: &CredentialRequest| Err(reason.to_owned()));
            let options = toha::ApplyOptions {
                force: false,
                trusted: true,
            };
            let applied = with_credential_resolver(resolver, || {
                plan.apply(&target, options, &NoHooks).map_err(Box::new)
            });
            let Err(error) = applied else {
                panic!("a refused remote must stop the apply");
            };
            assert!(
                matches!(
                    &*error,
                    toha::ApplyError::Git(toha::git_target::GitTargetError::CredentialRefused {
                        reason: refused,
                        ..
                    }) if refused == reason
                ),
                "{error}"
            );
            assert!(error.to_string().contains(reason), "{error}");
            assert_eq!(fs::read_dir(root.join("target")).unwrap().count(), 0);
        }
        "declines" => {
            let unset = fetch_from(&main).unwrap_err().to_string();
            let unset_probe = probe(&with_user(&main));
            let resolver = recorder(|_: &CredentialRequest| Ok(None));
            with_credential_resolver(resolver.clone(), || {
                let declined = fetch_from(&main).unwrap_err().to_string();
                assert_eq!(declined, unset);
                assert_eq!(probe(&with_user(&main)), unset_probe);
            });
            assert!(!resolver.asked.lock().unwrap().is_empty());
        }
        "refuses" => {
            let reason = "the sample resolver refuses this remote";
            let resolver = recorder(move |_: &CredentialRequest| Err(reason.to_owned()));
            with_credential_resolver(resolver, || {
                let error = fetch_from(&main).unwrap_err();
                assert!(error.to_string().contains(reason), "{error}");
                let probed = probe(&with_user(&main));
                assert_eq!(probed, RemoteState::Refused(reason.into()));
            });
        }
        "scoped" => {
            let wrong_origin: Arc<dyn CredentialResolver> = grant(private(), port(&main) + 1);
            let wrong_repository: Arc<dyn CredentialResolver> =
                recorder(|request: &CredentialRequest| {
                    if request.path == "/other.git" {
                        Ok(Some(Credential::new("fixture-user", "fixture-password")))
                    } else {
                        Err("these credentials belong to another repository".into())
                    }
                });
            for (resolver, reason) in [
                (wrong_origin, "another origin"),
                (wrong_repository, "another repository"),
            ] {
                with_credential_resolver(resolver, || {
                    let error = fetch_from(&main).unwrap_err().to_string();
                    assert!(error.contains(reason), "{error}");
                    let RemoteState::Refused(probed) = probe(&with_user(&main)) else {
                        panic!("a refused origin must not be reached");
                    };
                    assert!(probed.contains(reason), "{probed}");
                });
            }
        }
        "rejected" | "echoed" => {
            let resolver = recorder(move |_: &CredentialRequest| {
                Ok(Some(Credential::new("fixture-user", rejected)))
            });
            let (error, probed) = with_credential_resolver(resolver, || {
                let error = fetch_from(&main).unwrap_err().to_string();
                let RemoteState::Unreachable(probed) = probe(&with_user(&main)) else {
                    panic!("a rejected credential must not reach the remote");
                };
                (error, probed)
            });
            // gix names an unexpected service announcement as a byte list.
            let listed = format!("{:?}", rejected.as_bytes());
            let listed = listed.trim_start_matches('[').trim_end_matches(']');
            for message in [&error, &probed] {
                assert!(!message.contains(rejected), "{message}");
                assert!(!message.contains(listed), "{message}");
                if scenario == "echoed" {
                    // The endpoint did echo the credential; Toha removed it.
                    assert!(message.contains("<redacted>"), "{message}");
                }
            }
        }
        "plain" => {
            let resolver = grant(private(), port(&main));
            with_credential_resolver(resolver.clone(), || {
                assert!(fetch_from(&main).is_err());
                assert!(matches!(
                    probe(&with_user(&main)),
                    RemoteState::Unreachable(_)
                ));
            });
            assert!(resolver.asked.lock().unwrap().is_empty());
        }
        "moved" | "redirected" => {
            let (first, second) = (port(&main), port(&urls[1]));
            // Grants each origin its own credential.
            let resolver = recorder(move |request: &CredentialRequest| {
                let auth = if request.port == first {
                    private()
                } else if request.port == second {
                    other()
                } else {
                    return Err("unknown origin".into());
                };
                Ok(Some(Credential::new(auth.username, auth.password)))
            });
            let (fetched, probed) = with_credential_resolver(resolver.clone(), || {
                (fetch_from(&main), probe(&with_user(&main)))
            });
            let asked: Vec<u16> = resolver
                .asked
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.port)
                .collect();
            if scenario == "moved" {
                // The challenge came from the origin the redirect reached.
                fetched.unwrap();
                assert_eq!(probed, RemoteState::HasHistory);
                assert_eq!(asked, [second, second]);
            } else {
                // gix follows no redirect of an authenticated request, so the
                // operation stops at the first origin.
                let error = fetched.unwrap_err().to_string();
                assert!(!error.contains("password"), "{error}");
                assert!(matches!(probed, RemoteState::Unreachable(_)), "{probed:?}");
                assert_eq!(asked, [first, first]);
            }
        }
        "concurrent" => {
            let barrier = std::sync::Barrier::new(3);
            let (first, second) = (port(&main), port(&urls[1]));
            let grants = [
                (grant(private(), first), &urls[0]),
                (grant(other(), second), &urls[1]),
            ];
            thread::scope(|scope| {
                let workers: Vec<_> = grants
                    .iter()
                    .enumerate()
                    .map(|(n, (resolver, url))| {
                        let barrier = &barrier;
                        let root = &root;
                        scope.spawn(move || {
                            with_credential_resolver(resolver.clone(), || {
                                barrier.wait();
                                let address = Address::Git {
                                    repo: with_user(url),
                                    reference: None,
                                    path: None,
                                };
                                fetch(&address, &root.join(format!("concurrent-{n}"))).unwrap();
                                assert_eq!(probe(&with_user(url)), RemoteState::HasHistory);
                            })
                        })
                    })
                    .collect();
                // An operation without a resolver keeps today's behaviour.
                let unresolved = scope.spawn(|| {
                    barrier.wait();
                    let address = Address::Git {
                        repo: with_user(&main),
                        reference: None,
                        path: None,
                    };
                    assert!(fetch(&address, &root.join("concurrent-none")).is_err());
                });
                for worker in workers {
                    worker.join().unwrap();
                }
                unresolved.join().unwrap();
            });
            for (resolver, url) in &grants {
                let asked = resolver.asked.lock().unwrap();
                assert!(!asked.is_empty());
                assert!(asked.iter().all(|r| r.port == port(url)), "{asked:?}");
            }
        }
        other => panic!("unknown scenario {other}"),
    }
}
