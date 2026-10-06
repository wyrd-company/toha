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
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 8192];
        let size = stream.read(&mut chunk).unwrap();
        if size == 0 {
            return;
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
    // rustls rejects TCP EOF without the TLS close_notify alert, even after a
    // complete HTTP response. OpenSSL can accept that truncated shutdown.
    stream.conn.send_close_notify();
    stream.flush().unwrap();
}
#[test]
fn fetches_https_git_repository() {
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
                    let conn = ServerConnection::new(Arc::new(config.clone())).unwrap();
                    serve(StreamOwned::new(conn, socket), &root_path);
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
    let fetched = worker
        .args(["--ignored", "--exact", "https_fetch_worker", "--nocapture"])
        .env("TOHA_HTTPS_FIXTURE_ROOT", root.path())
        .env(
            "TOHA_HTTPS_FIXTURE_URL",
            format!("https://localhost:{port}/remote.git"),
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.path().join("empty.gitconfig"))
        .env("GIT_SSL_CAINFO", &cert)
        .env("CURL_CA_BUNDLE", &cert)
        .env("NO_PROXY", "localhost,127.0.0.1")
        .env("no_proxy", "localhost,127.0.0.1")
        .output()
        .unwrap();
    let token = "sample-secret";
    let url = format!("https://user:{token}@localhost:{port}/remote.git");
    let add = support::isolated_command(root.path())
        .args(["templates", "add", &url])
        .current_dir(root.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.path().join("empty.gitconfig"))
        .env("NO_PROXY", "localhost,127.0.0.1")
        .env("no_proxy", "localhost,127.0.0.1")
        .env("GIT_SSL_CAINFO", &cert)
        .env("CURL_CA_BUNDLE", &cert)
        .output()
        .unwrap();

    let failed = support::isolated_command(root.path())
        .args(["templates", "add", &format!("{url}@absent")])
        .current_dir(root.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.path().join("empty.gitconfig"))
        .env("NO_PROXY", "localhost,127.0.0.1")
        .env("no_proxy", "localhost,127.0.0.1")
        .env("GIT_SSL_CAINFO", &cert)
        .env("CURL_CA_BUNDLE", &cert)
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
    let root = std::path::PathBuf::from(std::env::var_os("TOHA_HTTPS_FIXTURE_ROOT").unwrap());
    let address = Address::Git {
        repo: std::env::var("TOHA_HTTPS_FIXTURE_URL").unwrap(),
        reference: None,
        path: None,
    };
    let fetched = fetch(&address, &root.join("fetched")).unwrap();
    assert_eq!(fetched.reference_kind, RefKind::DefaultBranch);
    assert_eq!(fetched.commit.len(), 40);
    assert!(root.join("fetched/template.yml").is_file());
}
