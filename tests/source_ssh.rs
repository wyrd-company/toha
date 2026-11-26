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
    io::{BufRead, Write},
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
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

struct Server(Child, Option<thread::JoinHandle<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        if let Some(log) = self.1.take() {
            log.join().unwrap();
        }
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

/// Start a local `sshd` on an ephemeral port that accepts the keys in
/// `authorized`, with the host key `root/host` that `root/known_hosts` trusts.
/// Returns the server, its port and its log.
fn sshd(root: &Path, authorized: &Path) -> (Server, u16, std::path::PathBuf) {
    // Reserve an ephemeral port, then release it immediately before sshd binds.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = root.join("sshd.conf");
    fs::write(&config, format!(
        "ListenAddress 127.0.0.1\nPort {port}\nHostKey {}/host\nPidFile {}/pid\nAuthorizedKeysFile {}\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nLogLevel VERBOSE\n",
        root.display(), root.display(), authorized.display()
    )).unwrap();
    let host_key = fs::read_to_string(root.join("host.pub")).unwrap();
    fs::write(
        root.join("known_hosts"),
        format!("[127.0.0.1]:{port} {host_key}"),
    )
    .unwrap();
    let log_path = root.join("sshd.log");
    let sshd = std::env::var_os("TOHA_TEST_SSHD").unwrap_or_else(|| "/usr/sbin/sshd".into());
    drop(listener);
    let mut child = Command::new(sshd)
        .args(["-D", "-e", "-f"])
        .arg(&config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut log = fs::File::create(&log_path).unwrap();
    let (ready, started) = mpsc::channel();
    let drain = thread::spawn(move || {
        let mut ready = Some(ready);
        for line in std::io::BufReader::new(stderr).lines() {
            let line = line.unwrap();
            writeln!(log, "{line}").unwrap();
            if line.starts_with("Server listening on ")
                && let Some(ready) = ready.take()
            {
                let _ = ready.send(());
            }
        }
    });
    let server = Server(child, Some(drain));
    // The server reports readiness after bind; EOF reports a startup failure.
    assert!(
        started.recv().is_ok(),
        "sshd did not start: {}",
        fs::read_to_string(&log_path).unwrap()
    );
    (server, port, log_path)
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
    let (server, port, log_path) = sshd(&root, &root.join("client.pub"));
    let ssh_config = root.join("ssh_config");
    fs::write(&ssh_config, format!(
        "Host *\n  BatchMode yes\n  IdentitiesOnly yes\n  IdentityAgent none\n  StrictHostKeyChecking yes\n  UserKnownHostsFile {}/known_hosts\n  GlobalKnownHostsFile /dev/null\n",
        root.display()
    )).unwrap();
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
    drop(server);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("Accepted publickey"), "{log}");
    assert!(log.contains("Failed publickey"), "{log}");
}

#[test]
#[ignore = "subprocess helper for fetches_ssh_git_repository_with_key_authentication"]
fn ssh_fetch_worker() {
    let Some(root) = std::env::var_os("TOHA_SSH_FIXTURE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
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

/// A local `ssh-agent`, stopped on drop.
struct Agent(Child);
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A repository with one commit holding `name`, served bare at
/// `root/<name>.git`. Returns its commit.
fn served_repository(root: &Path, name: &str) -> String {
    let source = root.join(format!("{name}-source"));
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    git(&source, &["config", "user.name", "Test"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    fs::write(
        source.join("template.yml"),
        format!("name: {name}\ninterview: []\n"),
    )
    .unwrap();
    fs::create_dir(source.join("template")).unwrap();
    fs::write(source.join("template/.gitkeep"), "").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", name]);
    let bare = format!("{name}.git");
    git(
        root,
        &["clone", "-q", "--bare", source.to_str().unwrap(), &bare],
    );
    String::from_utf8(git(&source, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned()
}

/// Each key reaches its own repository, whatever path a client asks for, so
/// the repository a fetch receives names the key the server accepted. The
/// agent holds `alpha`, `beta`, `empty` and `stranger`, in that order, so a
/// client that offers every agent key signs in with `alpha`. The server does
/// not accept `stranger`, and the agent does not hold `absent`.
#[test]
#[ignore = "requires local OpenSSH sshd, ssh-agent, ssh-add and ssh-keygen; run task test:network on Unix"]
fn an_ssh_key_scope_offers_only_its_key() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for name in ["host", "alpha", "beta", "empty", "stranger", "absent"] {
        checked(
            Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "", "-f"])
                .arg(root.join(name)),
        );
    }
    let alpha = served_repository(&root, "alpha");
    let beta = served_repository(&root, "beta");
    git(&root, &["init", "-q", "--bare", "empty.git"]);
    let mut authorized = String::new();
    for name in ["alpha", "beta", "empty"] {
        let key = fs::read_to_string(root.join(format!("{name}.pub"))).unwrap();
        authorized.push_str(&format!(
            "restrict,command=\"git-upload-pack '{}/{name}.git'\" {key}",
            root.display()
        ));
    }
    fs::write(root.join("authorized_keys"), authorized).unwrap();
    let (server, port, log_path) = sshd(&root, &root.join("authorized_keys"));
    // Agent keys only: no IdentitiesOnly, so a client offers every agent key.
    let ssh_config = root.join("ssh_config");
    fs::write(&ssh_config, format!(
        "Host *\n  BatchMode yes\n  StrictHostKeyChecking yes\n  UserKnownHostsFile {}/known_hosts\n  GlobalKnownHostsFile /dev/null\n",
        root.display()
    )).unwrap();
    let socket = root.join("agent.sock");
    let agent = Agent(
        Command::new("ssh-agent")
            .arg("-D")
            .arg("-a")
            .arg(&socket)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "ssh-agent did not start"
        );
        thread::sleep(std::time::Duration::from_millis(20));
    }
    checked(
        Command::new("ssh-add")
            .arg("-q")
            .args(["alpha", "beta", "empty", "stranger"].map(|name| root.join(name)))
            .env("SSH_AUTH_SOCK", &socket),
    );
    let user = String::from_utf8(checked(Command::new("id").arg("-un"))).unwrap();
    let url = format!("ssh://{}@127.0.0.1:{port}/any/path.git", user.trim());
    // `SSH_AUTH_SOCK` names the agent, then a key names it while
    // `SSH_AUTH_SOCK` names none.
    for mode in ["auth-sock", "named-agent"] {
        let mut worker = Command::new(std::env::current_exe().unwrap());
        support::isolate(&mut worker, &root);
        worker
            .args([
                "--ignored",
                "--exact",
                "ssh_key_scope_worker",
                "--nocapture",
            ])
            .env("TOHA_SSH_FIXTURE_ROOT", &root)
            .env("TOHA_SSH_FIXTURE_URL", &url)
            .env("TOHA_SSH_FIXTURE_MODE", mode)
            .env("TOHA_SSH_ALPHA_COMMIT", &alpha)
            .env("TOHA_SSH_BETA_COMMIT", &beta)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("empty.gitconfig"))
            .env(
                "GIT_SSH_COMMAND",
                format!("ssh -F '{}'", ssh_config.display()),
            );
        match mode {
            "auth-sock" => worker.env("SSH_AUTH_SOCK", &socket),
            _ => worker.env("SSH_AUTH_SOCK", root.join("no-agent.sock")),
        };
        let output = worker.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("ssh key scope worker passed"),
            "{mode}: {stdout}{}\nsshd: {}",
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(&log_path).unwrap()
        );
    }
    drop(agent);
    drop(server);
    let log = fs::read_to_string(&log_path).unwrap();
    for name in ["alpha", "beta", "empty"] {
        let listed = String::from_utf8(checked(
            Command::new("ssh-keygen")
                .arg("-lf")
                .arg(root.join(format!("{name}.pub"))),
        ))
        .unwrap();
        let fingerprint = listed.split_whitespace().nth(1).unwrap();
        assert!(
            log.contains(&format!("Accepted publickey for {} ", user.trim()))
                && log.contains(fingerprint),
            "{name} never signed in: {log}"
        );
    }
}

#[test]
#[ignore = "subprocess helper for an_ssh_key_scope_offers_only_its_key"]
fn ssh_key_scope_worker() {
    use toha::git_target::{RemoteState, probe};
    use toha::source::{SshKey, with_ssh_key};
    use toha::stop::{StopSignal, with_stop_signal};
    let Some(root) = std::env::var_os("TOHA_SSH_FIXTURE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let url = std::env::var("TOHA_SSH_FIXTURE_URL").unwrap();
    let named_agent = std::env::var("TOHA_SSH_FIXTURE_MODE").unwrap() == "named-agent";
    let alpha = std::env::var("TOHA_SSH_ALPHA_COMMIT").unwrap();
    let beta = std::env::var("TOHA_SSH_BETA_COMMIT").unwrap();
    let key = |name: &str| {
        let key =
            SshKey::parse(&fs::read_to_string(root.join(format!("{name}.pub"))).unwrap()).unwrap();
        if named_agent {
            key.with_agent(root.join("agent.sock"))
        } else {
            key
        }
    };
    let checkouts = TempDir::new_in(&root).unwrap();
    let fetched = |name: &str, reference: Option<&str>| {
        let address = Address::Git {
            repo: url.clone(),
            reference: reference.map(str::to_owned),
            path: None,
        };
        let dest = checkouts.path().join(name);
        fetch(&address, &dest).map(|fetched| fetched.commit)
    };

    if !named_agent {
        // Without a key, the client offers every agent key and signs in with
        // the first: today's behaviour.
        assert_eq!(fetched("default-before", None).unwrap(), alpha);
        assert_eq!(probe(&url), RemoteState::HasHistory);
    }

    // Two operations on two threads at once, each with its own key.
    let barrier = std::sync::Barrier::new(2);
    thread::scope(|scope| {
        let fetching = scope.spawn(|| {
            with_ssh_key(key("beta"), || {
                barrier.wait();
                fetched("beta", None)
            })
        });
        let probing = scope.spawn(|| {
            with_ssh_key(key("empty"), || {
                barrier.wait();
                probe(&url)
            })
        });
        assert_eq!(fetching.join().unwrap().unwrap(), beta);
        assert_eq!(probing.join().unwrap(), RemoteState::Empty);
    });

    // A commit lookup expands a short commit through the key's repository.
    let short = &beta[..10];
    let looked_up = with_ssh_key(key("beta"), || fetched("beta-commit", Some(short)));
    assert_eq!(looked_up.unwrap(), beta);

    // A stop signal runs the fetch on a Toha thread, which keeps the key.
    let stoppable = with_stop_signal(StopSignal::new(), || {
        with_ssh_key(key("beta"), || fetched("beta-stoppable", None))
    });
    assert_eq!(stoppable.unwrap(), beta);

    // A key the agent does not hold fails, naming the key, and no other key
    // is tried.
    let absent = key("absent");
    let named = format!("does not offer the key {absent}");
    let failed = with_ssh_key(absent.clone(), || fetched("absent", None));
    let message = failed.unwrap_err().to_string();
    assert!(message.contains(&named), "{message}");
    match with_ssh_key(absent, || probe(&url)) {
        RemoteState::Unreachable(message) => assert!(message.contains(&named), "{message}"),
        other => panic!("the probe with an absent key was {other:?}"),
    }

    if !named_agent {
        // Code after the scope keeps today's behaviour.
        assert_eq!(fetched("default-after", None).unwrap(), alpha);
    } else {
        // A key the server refuses fails, without falling back to the other
        // agent keys, which the server accepts. This runs last, as sshd may
        // turn the client away for a while after a refused sign-in.
        let refused = with_ssh_key(key("stranger"), || fetched("stranger", None));
        assert!(refused.is_err(), "a refused key fell back to {refused:?}");
        let probed = with_ssh_key(key("stranger"), || probe(&url));
        assert!(
            matches!(probed, RemoteState::Unreachable(_)),
            "a refused key fell back to {probed:?}"
        );
    }
    println!("ssh key scope worker passed");
}
