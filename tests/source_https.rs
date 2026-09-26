//! A local smart-HTTP Git endpoint over TLS exercises the real HTTPS transport.
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
    // The CA is provided only to this test process; no real user Git config is used.
    let address = Address::Git {
        repo: format!("https://localhost:{port}/remote.git"),
        reference: None,
        path: None,
    };
    // gix reads its own Git configuration. A local environment override supplies the CA.
    unsafe {
        std::env::set_var("HOME", root.path().join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.path().join("config"));
        std::env::set_var("XDG_DATA_HOME", root.path().join("data"));
        std::env::set_var("XDG_CACHE_HOME", root.path().join("cache"));
        std::env::set_var(
            "TOHA_USER_CONFIG",
            root.path().join("config/toha/config.yml"),
        );
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_SSL_CAINFO", &cert);
        std::env::set_var("CURL_CA_BUNDLE", &cert);
    }
    let fetched = fetch(&address, &root.path().join("fetched"));
    unsafe {
        for key in [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "TOHA_USER_CONFIG",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_SSL_CAINFO",
            "CURL_CA_BUNDLE",
        ] {
            std::env::remove_var(key);
        }
    }
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let fetched = fetched.unwrap();
    assert_eq!(fetched.reference_kind, RefKind::DefaultBranch);
    assert_eq!(fetched.commit.len(), 40);
    assert!(root.path().join("fetched/template.yml").is_file());
}
