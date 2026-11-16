// ---
// relationships:
//   implements: architecture
// ---
//! A stop signal ends a fetch, and a remote probe, that a stalled Git remote
//! holds, and stops an apply in its file loop.
#[allow(dead_code)]
mod support;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
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

/// Written by the endpoint when it holds a request.
const HELD: &str = "held";
/// Written by the worker to let the endpoint close the held request.
const RELEASE: &str = "release";
/// How long a stopped call may take to return: the signal wakes it at once,
/// and this bound only absorbs a slow machine.
const SOON: Duration = Duration::from_secs(5);

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
/// request without sending a byte until the worker writes [`RELEASE`], then
/// closes it unanswered.
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
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
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

fn serve(mut stream: TcpStream, root: &Path, stall: Stall) {
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 8192];
        let size = stream.read(&mut chunk).unwrap_or(0);
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
    let held = match stall {
        Stall::Refs => path.ends_with("/info/refs"),
        Stall::Pack => path.ends_with("/git-upload-pack"),
        Stall::Nothing => false,
    };
    if held {
        fs::write(root.join(HELD), "").unwrap();
        wait_for(Duration::from_secs(60), || root.join(RELEASE).exists());
        // Closed with no response, as a remote that gives up does.
        return;
    }
    let mut child = Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("REQUEST_METHOD", &method)
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
    let response = format!(
        "HTTP/1.1 {status}\r\n{header_lines}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
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

/// Serve a stalling remote and run `stop_worker` with `scenario` against it.
/// The worker is its own process, so its Git configuration stays isolated.
fn run_scenario(scenario: &str, stall: Stall) {
    let root = remote();
    let endpoint = Endpoint::start(root.path(), stall);
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root.path());
    let output = worker
        .args(["--ignored", "--exact", "stop_worker", "--nocapture"])
        .env("GIT_CONFIG_GLOBAL", root.path().join("fixture.gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env("TOHA_STOP_SCENARIO", scenario)
        .env("TOHA_STOP_ROOT", root.path())
        .env("TOHA_STOP_URL", endpoint.url())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // A failed worker may not have released the endpoint.
    fs::write(root.path().join(RELEASE), "").unwrap();
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
    run_scenario("fetch", Stall::Pack);
}

#[test]
fn a_template_resolution_held_by_a_stalled_remote_ends_soon_after_the_stop_signal() {
    run_scenario("resolve", Stall::Pack);
}

#[test]
fn a_remote_probe_held_by_a_stalled_remote_ends_soon_after_the_stop_signal() {
    run_scenario("probe", Stall::Refs);
}

#[test]
fn a_signal_that_never_fires_fetches_as_before() {
    run_scenario("unstopped", Stall::Nothing);
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
/// operation returned and how long after the signal it returned. Panics when
/// the operation returns before the signal, or not soon after it, or when the
/// worker thread does not end.
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

/// Release the held request, and wait until `folder` has no entry: the
/// abandoned transfer ended and removed what it fetched.
fn released_leaves_nothing_in(root: &Path, folder: &Path) {
    fs::write(root.join(RELEASE), "").unwrap();
    assert!(
        wait_for(Duration::from_secs(30), || fs::read_dir(folder)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true)),
        "the stopped fetch left {:?} in {}",
        fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        folder.display()
    );
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
    let address = Address::Git {
        repo: url.clone(),
        reference: None,
        path: None,
    };
    let out = root.join("out");
    fs::create_dir(&out).unwrap();
    match scenario.as_str() {
        "fetch" => {
            let dest = out.join("fetched");
            let target = dest.clone();
            let result = stop_while_held(&root, move || fetch(&address, &target));
            assert!(matches!(result, Err(SourceError::Stopped)), "{result:?}");
            assert!(!dest.exists());
            released_leaves_nothing_in(&root, &out);
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
            let sources = fs::read_dir(paths.cache.join("sources"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 1, "{sources:?}");
            released_leaves_nothing_in(&root, &sources[0]);
        }
        "probe" => {
            let result = stop_while_held(&root, move || probe(&url));
            assert_eq!(result, RemoteState::Stopped);
        }
        "unstopped" => {
            // The fetch runs on a Toha thread and is moved into place.
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
