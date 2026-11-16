// ---
// relationships:
//   verifies: architecture
// ---
//! Malformed addresses fail before contacting a remote, without exposing userinfo.
use toha::{
    git_target::{RemoteState, probe},
    source::{Address, fetch},
};

#[test]
fn escaped_credentials_stay_out_of_fetch_and_probe_parse_errors() {
    for password in [
        "it's-secret",
        "a\"secret",
        r"a\secret",
        "café-secret",
        "ü'\"\\secret",
    ] {
        for url in [
            format!("https://deploy:{password}@host.example:abc/o/r.git"),
            format!("ssh://deploy:{password}@host.example"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let address = Address::Git {
                repo: url.clone(),
                reference: None,
                path: None,
            };
            let error = fetch(&address, &root.path().join("clone")).unwrap_err();
            let RemoteState::Unreachable(probe_error) = probe(&url) else {
                panic!("malformed address must fail probing");
            };
            for message in [error.to_string(), format!("{error:?}"), probe_error] {
                assert!(message.contains("host.example"), "cause lost: {message}");
                assert!(!message.contains("deploy"), "userinfo leaked: {message}");
                for form in [
                    password.to_owned(),
                    password.escape_debug().to_string(),
                    password.as_bytes().escape_ascii().to_string(),
                ] {
                    assert!(!message.contains(&form), "password leaked: {message}");
                }
            }
        }
    }
}
