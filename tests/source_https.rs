// ---
// relationships:
//   implements: architecture
// ---
//! A local smart-HTTP Git endpoint over TLS exercises the real HTTPS transport.
#[allow(dead_code)]
mod support;
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
        Arc,
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
fn serve(mut stream: StreamOwned<ServerConnection, std::net::TcpStream>, root: &Path) {
    if serve_http(&mut stream, root) {
        // rustls rejects TCP EOF without the TLS close_notify alert, even after a
        // complete HTTP response. OpenSSL can accept that truncated shutdown.
        stream.conn.send_close_notify();
        stream.flush().unwrap();
    }
}

fn serve_http(stream: &mut (impl Read + Write), root: &Path) -> bool {
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
    for line in lines {
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

fn run_https_fixture(trust: Trust) {
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let root_path = root.path().to_path_buf();
    let server = thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((socket, _)) => {
                    // Accepted sockets inherit nonblocking mode on some platforms.
                    socket.set_nonblocking(false).unwrap();
                    if matches!(trust, Trust::Http) {
                        let mut socket = socket;
                        serve_http(&mut socket, &root_path);
                    } else {
                        let conn = ServerConnection::new(Arc::new(config.clone())).unwrap();
                        serve(StreamOwned::new(conn, socket), &root_path);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        }
    });
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
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
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
