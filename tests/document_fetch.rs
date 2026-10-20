// ---
// relationships:
//   verifies: template-composition
// ---
#[allow(dead_code)]
mod support;
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    sync::Arc,
};

fn checked(command: &mut Command) -> Vec<u8> {
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn run_response(make_body: impl FnOnce(&str) -> Vec<u8>) -> (std::process::Output, bool) {
    let root = tempfile::tempdir().unwrap();
    let cert = root.path().join("cert.pem");
    let key = root.path().join("key.pem");
    checked(Command::new("openssl").args([
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
    ]));
    let der = checked(Command::new("openssl").args([
        "x509",
        "-in",
        cert.to_str().unwrap(),
        "-outform",
        "DER",
    ]));
    let key_der = checked(Command::new("openssl").args([
        "pkcs8",
        "-topk8",
        "-nocrypt",
        "-in",
        key.to_str().unwrap(),
        "-outform",
        "DER",
    ]));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(der)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!(
        "https://localhost:{}/schema",
        listener.local_addr().unwrap().port()
    );
    let body = make_body(&uri);
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(Arc::new(config)).unwrap(), tcp);
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&chunk[..n]);
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = stream.write_all(&body);
        stream.conn.send_close_notify();
        let _ = stream.flush();
    });
    let template = root.path().join("template");
    let target = root.path().join("target");
    fs::create_dir_all(template.join("template")).unwrap();
    fs::write(template.join("template.yml"),format!("name: sample\ninterview:\n  - id: catalog\n    type: document\n    prompt: Catalog\n    schema: {uri}\n    input: catalog.json\n")).unwrap();
    fs::write(template.join("template/out.txt"), "{{ catalog.title }}").unwrap();
    let input = root.path().join("input.json");
    fs::write(&input, json!({"$schema":uri,"title":"Fetched"}).to_string()).unwrap();
    let out = support::isolated_command(root.path())
        .arg("apply")
        .arg(support::folder_address(&template))
        .arg(&target)
        .arg("--input")
        .arg(input)
        .env("SSL_CERT_FILE", &cert)
        .env("GIT_SSL_CAINFO", &cert)
        .env("CURL_CA_BUNDLE", &cert)
        .env("NO_PROXY", "localhost,127.0.0.1")
        .env("no_proxy", "localhost,127.0.0.1")
        .output()
        .unwrap();
    assert!(
        server.join().is_ok(),
        "client stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let written = target.join("out.txt").exists();
    if written {
        assert_eq!(
            fs::read_to_string(target.join("out.txt")).unwrap(),
            "Fetched"
        );
    }
    (out, written)
}
#[test]
fn hosted_https_schema_is_fetched_with_explicit_tls_roots() {
    let (out, written) = run_response(|uri| {
        json!({"$id":uri,"type":"object","required":["title"]})
            .to_string()
            .into_bytes()
    });
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(written);
}
#[test]
fn hosted_schema_with_wrong_id_stops_before_writes() {
    let (out, written) = run_response(|_| {
        json!({"$id":"https://schemas.example.test/other","type":"object"})
            .to_string()
            .into_bytes()
    });
    assert_eq!(out.status.code(), Some(1));
    assert!(!written);
    assert!(String::from_utf8_lossy(&out.stdout).contains("$id does not equal"));
}
#[test]
fn hosted_schema_over_one_mib_stops_before_writes() {
    let (out, written) = run_response(|_| vec![b' '; 1024 * 1024 + 1]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!written);
    assert!(String::from_utf8_lossy(&out.stdout).contains("exceeds 1 MiB"));
}
#[test]
fn hosted_invalid_json_schema_stops_before_writes() {
    let (out, written) = run_response(|uri| json!({"$id":uri,"type":42}).to_string().into_bytes());
    assert_eq!(out.status.code(), Some(1));
    assert!(!written);
}
#[test]
fn hosted_non_json_response_stops_before_writes() {
    let (out, written) = run_response(|_| b"not json".to_vec());
    assert_eq!(out.status.code(), Some(1));
    assert!(!written);
}
