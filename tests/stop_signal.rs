// ---
// relationships:
//   implements: architecture
// ---
//! A stop signal returns a fetch, and a remote probe, that a stalled Git
//! remote holds, and stops an apply in its file loop.
//!
//! The transfer a stop abandons keeps its thread, and its connection or
//! `ssh`/`git-upload-pack` process, until the remote sends or closes, or Git's
//! `http.lowSpeedLimit` ends it (wyrd-company/toha#78). These tests show each
//! of those ways, and that the transfer then writes nothing more and removes
//! its folder.
#[allow(dead_code)]
mod support;
use std::{
    fs,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use toha::stop::{StopSignal, with_stop_signal};

/// Written by the remote when it holds a request.
const HELD: &str = "held";
/// Written by the worker to let the remote act on the held request: `close`
/// closes it unanswered, `send` answers it with Git protocol and keeps the
/// connection or process open.
const RELEASE: &str = "release";
/// Written by the HTTP remote once Toha has closed the held connection.
const CLIENT_CLOSED: &str = "client-closed";
/// Written by a stalled `ssh` or `git-upload-pack` program: its process id.
const REMOTE_PID: &str = "remote.pid";
/// How long a stopped call may take to return: the signal wakes it at once,
/// and this bound only absorbs a slow machine.
const SOON: Duration = Duration::from_secs(5);
/// How long the abandoned transfer may take to end once the remote acted.
const ENDED: Duration = Duration::from_secs(30);
/// Git's low-speed limit in the low-speed scenario: a transfer below 1000
/// bytes a second for 2 seconds ends.
const LOW_SPEED: &str = "[http]\n\tlowSpeedLimit = 1000\n\tlowSpeedTime = 2\n";
/// A Git remote reached through a program, as `ssh` and `git-upload-pack`
/// are: it sends nothing until the worker writes [`RELEASE`], then answers as
/// `git-upload-pack` for `remote.git` does (`send`), or exits (`close`).
const STALLED_PROGRAM: &str = r#"#!/bin/sh
root="${TOHA_STOP_ROOT:?}"
echo $$ > "$root/remote.pid"
: > "$root/held"
while [ ! -s "$root/release" ]; do sleep 0.05; done
case "$(cat "$root/release")" in
send) exec git upload-pack "$root/remote.git" ;;
*) exit 0 ;;
esac
"#;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Wait until `ready` holds, for at most `limit`.
fn wait_for(limit: Duration, ready: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if ready() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    ready()
}

/// The request an endpoint holds instead of answering.
#[derive(Clone, Copy)]
enum Stall {
    /// The ref advertisement, which a probe asks for first.
    Refs,
    /// The pack request, which a fetch makes after the advertisement.
    Pack,
    /// Nothing: the remote answers every request.
    Nothing,
}

/// A smart-HTTP Git endpoint for `root/remote.git` that holds one kind of
/// request without sending a byte, as [`hold`] describes.
struct Endpoint {
    port: u16,
    stop: Arc<AtomicBool>,
    server: thread::JoinHandle<()>,
}

impl Endpoint {
    fn start(root: &Path, stall: Stall) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let root = root.to_path_buf();
        let server = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        let root = root.clone();
                        thread::spawn(move || serve(socket, &root, stall));
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self { port, stop, server }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/remote.git", self.port)
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.join().unwrap();
    }
}

/// One request an endpoint read.
struct Request {
    method: String,
    path: String,
    query: String,
    content_type: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 8192];
        let size = stream.read(&mut chunk).unwrap_or(0);
        if size == 0 {
            return None;
        }
        request.extend_from_slice(&chunk[..size]);
        if let Some(pos) = request.windows(4).position(|v| v == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let mut lines = headers.lines();
    let mut words = lines.next().unwrap().split_whitespace();
    let method = words.next().unwrap().to_owned();
    let target = words.next().unwrap().to_owned();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut content_length = 0;
    let mut content_type = String::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap();
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = value.trim().into();
            }
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
    Some(Request {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        content_type,
        body: request[header_end..header_end + content_length].to_vec(),
    })
}

fn serve(mut stream: TcpStream, root: &Path, stall: Stall) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let held = match stall {
        Stall::Refs => request.path.ends_with("/info/refs"),
        Stall::Pack => request.path.ends_with("/git-upload-pack"),
        Stall::Nothing => false,
    };
    if held {
        fs::write(root.join(HELD), "").unwrap();
        hold(stream, root, &request);
        return;
    }
    stream.write_all(&answer(root, &request, true)).unwrap();
    stream.flush().unwrap();
}

/// Hold `stream` unanswered and watch it. When Toha closes it, write
/// [`CLIENT_CLOSED`]. When the worker writes [`RELEASE`], act on it once:
/// `close` closes the remote's side unanswered, `send` answers the request and
/// keeps the connection open.
fn hold(mut stream: TcpStream, root: &Path, request: &Request) {
    stream.set_nonblocking(true).unwrap();
    let mut pending = true;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(120) {
        let mut buffer = [0; 1024];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if pending && let Ok(action) = fs::read_to_string(root.join(RELEASE)) {
            match action.trim() {
                "close" => {
                    let _ = stream.shutdown(Shutdown::Write);
                }
                "send" => {
                    stream.set_nonblocking(false).unwrap();
                    stream.write_all(&answer(root, request, false)).unwrap();
                    stream.flush().unwrap();
                    stream.set_nonblocking(true).unwrap();
                }
                other => panic!("unknown release {other:?}"),
            }
            pending = false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    fs::write(root.join(CLIENT_CLOSED), "").unwrap();
}

/// The response `git http-backend` gives `request`, closing the connection
/// after it when `close`.
fn answer(root: &Path, request: &Request, close: bool) -> Vec<u8> {
    let mut child = Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("REQUEST_METHOD", &request.method)
        .env("PATH_INFO", &request.path)
        .env("QUERY_STRING", &request.query)
        .env("CONTENT_TYPE", &request.content_type)
        .env("CONTENT_LENGTH", request.body.len().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&request.body)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let separator = output
        .stdout
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| {
            output
                .stdout
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|p| p + 2)
        })
        .unwrap();
    let (head, body) = output.stdout.split_at(separator);
    let head = String::from_utf8_lossy(head);
    let status = head
        .lines()
        .find_map(|line| line.strip_prefix("Status: "))
        .unwrap_or("200 OK")
        .trim()
        .to_owned();
    let header_lines: String = head
        .lines()
        .filter(|line| !line.starts_with("Status:") && !line.trim().is_empty())
        .map(|line| format!("{}\r\n", line.trim_end_matches('\r')))
        .collect();
    let connection = if close { "Connection: close\r\n" } else { "" };
    let mut response = format!(
        "HTTP/1.1 {status}\r\n{header_lines}Content-Length: {}\r\n{connection}\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

/// A new root holding a template repository at `remote.git`.
fn remote() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    fs::write(source.join("template.yml"), "name: sample\ninterview: []\n").unwrap();
    fs::create_dir(source.join("template")).unwrap();
    fs::write(source.join("template/.gitkeep"), "").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", "initial"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            "--bare",
            source.to_str().unwrap(),
            "remote.git",
        ],
    );
    root
}

/// What a scenario's worker runs against.
#[derive(Clone, Copy)]
enum Remote {
    /// The HTTP endpoint, with Git's global configuration `config`.
    Http(Stall, &'static str),
    /// [`STALLED_PROGRAM`] as the `ssh` program.
    #[cfg(unix)]
    Ssh,
    /// [`STALLED_PROGRAM`] as `git-upload-pack`, for a `file://` remote.
    #[cfg(unix)]
    File,
}

/// Serve the remote and run `stop_worker` with `scenario` against it. The
/// worker is its own process, so its Git configuration, `PATH` and threads
/// stay its own.
fn run_scenario(scenario: &str, remote_kind: Remote) {
    let root = remote();
    let stall = match remote_kind {
        Remote::Http(stall, _) => stall,
        #[cfg(unix)]
        _ => Stall::Nothing,
    };
    let endpoint = Endpoint::start(root.path(), stall);
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root.path());
    let config = root.path().join("fixture.gitconfig");
    fs::write(
        &config,
        match remote_kind {
            Remote::Http(_, config) => config,
            #[cfg(unix)]
            _ => "",
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        for name in ["stalled-ssh", "git-upload-pack"] {
            fs::write(bin.join(name), STALLED_PROGRAM).unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        match remote_kind {
            Remote::Ssh => {
                worker.env("GIT_SSH_COMMAND", bin.join("stalled-ssh"));
            }
            Remote::File => {
                let path = std::env::var_os("PATH").unwrap_or_default();
                let mut paths = vec![bin.clone()];
                paths.extend(std::env::split_paths(&path));
                worker.env("PATH", std::env::join_paths(paths).unwrap());
            }
            Remote::Http(..) => {}
        }
    }
    let output = worker
        .args(["--ignored", "--exact", "stop_worker", "--nocapture"])
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env("TOHA_STOP_SCENARIO", scenario)
        .env("TOHA_STOP_ROOT", root.path())
        .env("TOHA_STOP_URL", endpoint.url())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // A worker that failed may not have released the remote.
    if fs::read_to_string(root.path().join(RELEASE)).is_err() {
        fs::write(root.path().join(RELEASE), "close").unwrap();
    }
    endpoint.stop();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}{stderr}"
    );
    for line in stdout.lines().filter(|line| line.contains("stop signal")) {
        println!("{scenario}: {line}");
    }
}

#[test]
fn a_fetch_held_by_a_stalled_remote_ends_soon_after_the_stop_signal() {
    run_scenario("fetch", Remote::Http(Stall::Pack, ""));
}

#[test]
fn a_stopped_fetch_ends_its_transfer_when_the_remote_answers() {
    run_scenario("fetch-answered", Remote::Http(Stall::Pack, ""));
}

#[test]
fn a_stopped_fetch_ends_its_transfer_at_the_low_speed_limit() {
    run_scenario("fetch-low-speed", Remote::Http(Stall::Pack, LOW_SPEED));
}

#[test]
fn a_template_resolution_held_by_a_stalled_remote_ends_soon_after_the_stop_signal() {
    run_scenario("resolve", Remote::Http(Stall::Pack, ""));
}

#[test]
fn a_remote_probe_held_by_a_stalled_remote_ends_soon_after_the_stop_signal() {
    run_scenario("probe", Remote::Http(Stall::Refs, ""));
}

#[test]
fn a_signal_that_never_fires_fetches_as_before() {
    run_scenario("unstopped", Remote::Http(Stall::Nothing, ""));
}

#[cfg(unix)]
#[test]
fn a_stopped_ssh_fetch_ends_its_transfer_when_ssh_exits() {
    run_scenario("program-exits", Remote::Ssh);
}

#[cfg(unix)]
#[test]
fn a_stopped_ssh_fetch_ends_its_transfer_when_ssh_sends() {
    run_scenario("program-sends", Remote::Ssh);
}

#[cfg(unix)]
#[test]
fn a_stopped_file_fetch_ends_its_transfer_when_git_upload_pack_exits() {
    run_scenario("program-exits", Remote::File);
}

#[cfg(unix)]
#[test]
fn a_stopped_file_fetch_ends_its_transfer_when_git_upload_pack_sends() {
    run_scenario("program-sends", Remote::File);
}

#[test]
// `ApplyError` is large, as elsewhere in the crate.
#[allow(clippy::result_large_err)]
fn a_stop_during_the_apply_file_loop_writes_no_later_file() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("template");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template.yml"), "name: sample\ninterview: []\n").unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(folder.join("template").join(name), name).unwrap();
    }
    let template = toha::Template::load(&folder).unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();
    let target = toha::staging::canonical_target(&target).unwrap();
    let toha::Interview::Complete(completed) = toha::Interview::start(
        &template,
        toha::Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
            context: toha::context::InvocationContext::for_target(target.clone()),
        },
    )
    .unwrap() else {
        panic!("no interview questions")
    };
    let plan = toha::Plan::build(&template, &completed, &target).unwrap();
    assert_eq!(plan.files.len(), 3);
    // The caller's request ends while the first file is written.
    let signal = StopSignal::new();
    let mut written = vec![];
    let result = with_stop_signal(signal.clone(), || {
        plan.apply_reporting(
            &target,
            toha::ApplyOptions {
                force: false,
                trusted: true,
            },
            &toha::hook::ProcessRunner,
            &mut |path| {
                written.push(path.to_string());
                signal.stop();
            },
        )
    });
    assert!(
        matches!(result, Err(toha::ApplyError::Stopped)),
        "{result:?}"
    );
    assert_eq!(written.len(), 1, "{written:?}");
    let on_disk: Vec<_> = fs::read_dir(target.as_path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(on_disk, written, "no file after the first was written");
}

/// Run `operation` on a worker thread inside a stop signal's scope, wait until
/// the remote holds its request, fire the signal, and return what the
/// operation returned. Panics when the operation returns before the signal,
/// or not soon after it, or when the worker thread does not end.
fn stop_while_held<T: Send + 'static>(
    root: &Path,
    operation: impl FnOnce() -> T + Send + 'static,
) -> T {
    let signal = StopSignal::new();
    let (sender, receiver) = mpsc::channel();
    let scoped = signal.clone();
    let worker = thread::spawn(move || {
        sender.send(with_stop_signal(scoped, operation)).unwrap();
    });
    assert!(
        wait_for(Duration::from_secs(30), || root.join(HELD).exists()),
        "the remote never held a request"
    );
    // The remote holds the request: without the signal the operation waits.
    thread::sleep(Duration::from_millis(300));
    assert!(
        matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "the operation returned while the remote held its request"
    );
    let stopped = Instant::now();
    signal.stop();
    let result = receiver
        .recv_timeout(SOON)
        .expect("the operation did not end soon after the stop signal");
    let elapsed = stopped.elapsed();
    worker.join().expect("the worker thread returns");
    println!("returned {elapsed:?} after the stop signal");
    assert!(elapsed < SOON);
    result
}

/// The threads named `toha-remote` in this process: Toha's remote thread and,
/// over HTTP, gix's curl thread, which inherits its name. Counted on Linux.
fn remote_threads() -> Option<usize> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    Some(
        fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|entry| fs::read_to_string(entry.ok()?.path().join("comm")).ok())
            .filter(|name| name.trim() == "toha-remote")
            .count(),
    )
}

/// The stalled program's process, while it runs.
fn remote_process(root: &Path) -> Option<u32> {
    let pid = fs::read_to_string(root.join(REMOTE_PID)).ok()?;
    let pid = pid.trim().to_owned();
    let alive = Command::new("kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    alive.then(|| pid.parse().unwrap())
}

/// How the remote is reached, for what a held transfer keeps.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Connection,
    Process,
}

/// The documented limit: after the call returned, the abandoned transfer
/// still holds its thread and its connection or process.
fn assert_still_held(root: &Path, held: Held) {
    if let Some(threads) = remote_threads() {
        assert!(threads >= 1, "the abandoned transfer keeps its thread");
    }
    match held {
        Held::Connection => assert!(
            !root.join(CLIENT_CLOSED).exists(),
            "the abandoned transfer keeps its connection"
        ),
        Held::Process => assert!(
            remote_process(root).is_some(),
            "the abandoned transfer keeps its process"
        ),
    }
}

/// Once the remote acted, the abandoned transfer ends: its threads end, its
/// connection closes or its process ends, and `folder` is left empty.
fn assert_transfer_ended(root: &Path, folder: &Path, held: Held) {
    let empty = || {
        fs::read_dir(folder)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true)
    };
    let released = || match held {
        Held::Connection => root.join(CLIENT_CLOSED).exists(),
        Held::Process => remote_process(root).is_none(),
    };
    let ended = || remote_threads().is_none_or(|threads| threads == 0);
    wait_for(ENDED, || empty() && released() && ended());
    assert!(
        released(),
        "the transfer's connection or process outlived the remote"
    );
    assert!(ended(), "the transfer's threads outlived the remote");
    assert!(
        empty(),
        "the stopped transfer left {:?} in {}",
        fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        folder.display()
    );
    println!("the transfer ended after the remote acted; its folder is empty");
}

#[test]
#[ignore = "subprocess helper for the stop signal scenarios"]
fn stop_worker() {
    use toha::{
        git_target::{RemoteState, probe},
        resolve::{Paths, ResolveError, load_context, resolve_template},
        source::{Address, SourceError, fetch},
    };
    let Ok(scenario) = std::env::var("TOHA_STOP_SCENARIO") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("TOHA_STOP_ROOT").unwrap());
    let url = std::env::var("TOHA_STOP_URL").unwrap();
    let out = root.join("out");
    fs::create_dir(&out).unwrap();
    let release = |action: &str| fs::write(root.join(RELEASE), action).unwrap();
    // Fetch `repo` into `out`, stop it while the remote holds it, and check
    // that the call returned and the transfer still holds what the
    // documentation says.
    let stop_fetch = |repo: String, held: Held| {
        let address = Address::Git {
            repo,
            reference: None,
            path: None,
        };
        let dest = out.join("fetched");
        let target = dest.clone();
        let result = stop_while_held(&root, move || fetch(&address, &target));
        assert!(matches!(result, Err(SourceError::Stopped)), "{result:?}");
        assert!(!dest.exists());
        assert_still_held(&root, held);
    };
    match scenario.as_str() {
        "fetch" => {
            stop_fetch(url, Held::Connection);
            release("close");
            assert_transfer_ended(&root, &out, Held::Connection);
        }
        "fetch-answered" => {
            stop_fetch(url, Held::Connection);
            release("send");
            assert_transfer_ended(&root, &out, Held::Connection);
        }
        "fetch-low-speed" => {
            // The remote never acts: Git's low-speed limit ends the transfer.
            stop_fetch(url, Held::Connection);
            assert_transfer_ended(&root, &out, Held::Connection);
        }
        "program-exits" | "program-sends" => {
            let repo = if std::env::var_os("GIT_SSH_COMMAND").is_some() {
                "ssh://example.invalid/remote.git".to_owned()
            } else {
                format!("file://{}", root.join("remote.git").display())
            };
            stop_fetch(repo, Held::Process);
            release(if scenario == "program-sends" {
                "send"
            } else {
                "close"
            });
            assert_transfer_ended(&root, &out, Held::Process);
        }
        "resolve" => {
            let paths = Paths {
                system_config: out.join("system/config.yml"),
                user_config: out.join("user/config.yml"),
                local_config_override: None,
                system_data: out.join("system/data"),
                user_data: out.join("user/data"),
                cache: out.join("cache"),
                state: out.join("state"),
                home: root.join("home"),
            };
            let (config, registry, _) = load_context(&paths, &root).unwrap();
            let (cwd, resolving) = (root.clone(), paths.clone());
            let result = stop_while_held(&root, move || {
                resolve_template(&url, &config, &registry, &resolving, &cwd)
            });
            assert!(matches!(result, Err(ResolveError::Stopped)), "{result:?}");
            assert_still_held(&root, Held::Connection);
            let sources = fs::read_dir(paths.cache.join("sources"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 1, "{sources:?}");
            release("close");
            assert_transfer_ended(&root, &sources[0], Held::Connection);
        }
        "probe" => {
            let result = stop_while_held(&root, move || probe(&url));
            assert_eq!(result, RemoteState::Stopped);
            assert_still_held(&root, Held::Connection);
            release("close");
            assert_transfer_ended(&root, &out, Held::Connection);
        }
        "unstopped" => {
            // The fetch runs on a Toha thread and is moved into place.
            let address = Address::Git {
                repo: url,
                reference: None,
                path: None,
            };
            let dest = out.join("fetched");
            let fetched = with_stop_signal(StopSignal::new(), || fetch(&address, &dest));
            let fetched = fetched.unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(fetched.commit.len(), 40);
            assert!(dest.join("template.yml").is_file());
            assert_eq!(fs::read_dir(&out).unwrap().count(), 1, "only the clone");
        }
        other => panic!("unknown scenario {other}"),
    }
}
