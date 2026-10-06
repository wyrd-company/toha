// ---
// relationships:
//   implements: architecture
// ---
//! Opt-in local OpenSSH authentication, without operator keys or hosted services.
#![cfg(unix)]
#[allow(dead_code)]
mod support;

use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
use tempfile::TempDir;
use toha::source::{Address, RefKind, fetch};

fn checked(command: &mut Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    checked(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("empty.gitconfig")),
    )
}

#[test]
#[ignore = "requires local OpenSSH sshd and ssh-keygen; run task test:network on Unix"]
fn fetches_ssh_git_repository_with_key_authentication() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-b", "main"]);
    git(&source, &["config", "user.name", "Test"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    fs::write(source.join("template.yml"), "name: sample\ninterview: []\n").unwrap();
    fs::create_dir(source.join("template")).unwrap();
    fs::write(source.join("template/.gitkeep"), "").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    let expected_commit = String::from_utf8(git(&source, &["rev-parse", "HEAD"])).unwrap();
    git(
        &root,
        &["clone", "--bare", source.to_str().unwrap(), "remote.git"],
    );
    for name in ["host", "client", "wrong-client"] {
        checked(
            Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(root.join(name)),
        );
    }
    // Reserve an ephemeral port, then release it immediately before sshd binds.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = root.join("sshd.conf");
    fs::write(&config, format!(
        "ListenAddress 127.0.0.1\nPort {port}\nHostKey {}/host\nPidFile {}/pid\nAuthorizedKeysFile {}/client.pub\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nLogLevel VERBOSE\n",
        root.display(), root.display(), root.display()
    )).unwrap();
    let host_key = fs::read_to_string(root.join("host.pub")).unwrap();
    fs::write(
        root.join("known_hosts"),
        format!("[127.0.0.1]:{port} {host_key}"),
    )
    .unwrap();
    let ssh_config = root.join("ssh_config");
    fs::write(&ssh_config, format!(
        "Host *\n  BatchMode yes\n  IdentitiesOnly yes\n  IdentityAgent none\n  StrictHostKeyChecking yes\n  UserKnownHostsFile {}/known_hosts\n  GlobalKnownHostsFile /dev/null\n",
        root.display()
    )).unwrap();
    let log_path = root.join("sshd.log");
    let sshd = std::env::var_os("TOHA_TEST_SSHD").unwrap_or_else(|| "/usr/sbin/sshd".into());
    drop(listener);
    let mut server = Server(
        Command::new(sshd)
            .args(["-D", "-e", "-f"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log_path).unwrap())
            .spawn()
            .unwrap(),
    );
    // Startup polling is confined to this test fixture, with no product timeout.
    let mut ready = false;
    for _ in 0..100 {
        if server.0.try_wait().unwrap().is_some() {
            break;
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ready,
        "sshd did not start: {}",
        fs::read_to_string(&log_path).unwrap()
    );
    let user = String::from_utf8(checked(Command::new("id").arg("-un"))).unwrap();
    let url = format!(
        "ssh://{}@127.0.0.1:{port}{}/remote.git",
        user.trim(),
        root.display()
    );
    for (key, succeeds) in [("client", true), ("wrong-client", false)] {
        let mut worker = Command::new(std::env::current_exe().unwrap());
        support::isolate(&mut worker, &root);
        let output = worker
            .args(["--ignored", "--exact", "ssh_fetch_worker", "--nocapture"])
            .env("TOHA_SSH_FIXTURE_ROOT", &root)
            .env("TOHA_SSH_FIXTURE_URL", &url)
            .env("TOHA_SSH_FIXTURE_KEY", key)
            .env("TOHA_SSH_EXPECTED_COMMIT", expected_commit.trim())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("empty.gitconfig"))
            .env("GIT_SSH_VARIANT", "ssh")
            .env(
                "GIT_SSH_COMMAND",
                format!(
                    "ssh -F '{}' -i '{}'",
                    ssh_config.display(),
                    root.join(key).display()
                ),
            )
            .env_remove("SSH_AUTH_SOCK")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}\nsshd: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(&log_path).unwrap()
        );
        if succeeds {
            assert!(
                root.join(key)
                    .with_extension("checkout")
                    .join("template.yml")
                    .is_file()
            );
        }
    }
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("Accepted publickey"), "{log}");
    assert!(log.contains("Failed publickey"), "{log}");
}

#[test]
#[ignore = "subprocess helper for fetches_ssh_git_repository_with_key_authentication"]
fn ssh_fetch_worker() {
    let root = std::path::PathBuf::from(std::env::var_os("TOHA_SSH_FIXTURE_ROOT").unwrap());
    let key = std::env::var("TOHA_SSH_FIXTURE_KEY").unwrap();
    let address = Address::Git {
        repo: std::env::var("TOHA_SSH_FIXTURE_URL").unwrap(),
        reference: None,
        path: None,
    };
    let result = fetch(&address, &root.join(&key).with_extension("checkout"));
    if key == "client" {
        let fetched = result.unwrap();
        assert_eq!(fetched.reference_kind, RefKind::DefaultBranch);
        assert_eq!(
            fetched.commit,
            std::env::var("TOHA_SSH_EXPECTED_COMMIT").unwrap()
        );
    } else {
        assert!(
            result.is_err(),
            "an unrecognized SSH key must fail authentication"
        );
    }
}
