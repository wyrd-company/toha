// ---
// relationships:
//   implements: architecture
// ---
//! A stop signal ends a template load that a stalled HTTPS document schema
//! endpoint holds: the load returns within 2 s and its connection is closed
//! before the endpoint lets go, and no thread outlives it. Without a signal,
//! or with one that never fires, the load waits for the endpoint as before.
#[allow(dead_code)]
mod support;
use std::{
    fs,
    io::{ErrorKind, Read},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use toha::stop::{StopSignal, with_stop_signal};

/// Written by the endpoint when it accepts a connection.
const ACCEPTED: &str = "accepted";
/// Written by the endpoint once it holds the schema request.
const HELD: &str = "held";
/// Written by the worker to let the endpoint close the held connection.
const RELEASE: &str = "release";
/// Written by the endpoint when Toha closed the held connection, before
/// [`RELEASE`].
const CLIENT_CLOSED: &str = "client-closed";
/// The bound task 29 sets: the load returns, and its connection is closed,
/// within this time of the stop signal.
const BOUND: Duration = Duration::from_secs(2);

fn wait_for(limit: Duration, ready: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if ready() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    ready()
}

/// An HTTPS endpoint that accepts one connection, reads the client's first
/// TLS bytes and then sends nothing. It writes [`CLIENT_CLOSED`] if the
/// client closes the connection before [`RELEASE`], and closes it on
/// [`RELEASE`].
fn stalled_endpoint(root: &Path) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("https://{}/schema.json", listener.local_addr().unwrap());
    let root = root.to_owned();
    let server = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    fs::write(root.join(ACCEPTED), "").unwrap();
                    break stream;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if root.join(RELEASE).exists() || started.elapsed() > Duration::from_secs(60) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream.set_nonblocking(true).unwrap();
        let mut held = false;
        while !root.join(RELEASE).exists() && started.elapsed() < Duration::from_secs(60) {
            let mut buffer = [0; 4096];
            match stream.read(&mut buffer) {
                Ok(0) => {
                    fs::write(root.join(CLIENT_CLOSED), "").unwrap();
                    return;
                }
                Ok(_) if !held => {
                    held = true;
                    fs::write(root.join(HELD), "").unwrap();
                }
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => {
                    fs::write(root.join(CLIENT_CLOSED), "").unwrap();
                    return;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Released: the connection closes as the stream drops.
    });
    (uri, server)
}

/// Serve a stalled schema endpoint and run `schema_worker` with `scenario`
/// against it. The worker is its own process, so its proxy and certificate
/// environment stay isolated.
fn run_scenario(scenario: &str) {
    let root = tempfile::tempdir().unwrap();
    let (uri, server) = stalled_endpoint(root.path());
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root.path());
    let output = worker
        .args(["--ignored", "--exact", "schema_worker", "--nocapture"])
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env_remove("https_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("all_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("GIT_SSL_CAINFO")
        .env("TOHA_SCHEMA_SCENARIO", scenario)
        .env("TOHA_SCHEMA_ROOT", root.path())
        .env("TOHA_SCHEMA_URI", &uri)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // A worker that failed may not have released the endpoint.
    let _ = fs::write(root.path().join(RELEASE), "");
    server.join().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}{stderr}"
    );
    for line in stdout.lines().filter(|line| line.starts_with("observed:")) {
        println!("{scenario}: {line}");
    }
}

#[test]
fn a_stop_ends_a_template_load_held_by_a_stalled_schema_endpoint() {
    run_scenario("stopped");
}

#[test]
fn a_stopped_signal_loads_without_connecting_to_the_schema_endpoint() {
    run_scenario("stopped-before");
}

#[test]
fn without_a_signal_a_template_load_waits_for_the_schema_endpoint() {
    run_scenario("unstopped");
}

#[test]
fn a_signal_that_never_fires_leaves_the_schema_fetch_waiting() {
    run_scenario("never-fires");
}

/// A template whose document question names the schema at `uri`.
fn template(root: &Path, uri: &str) -> PathBuf {
    let folder = root.join("template");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template.yml"),
        format!(
            "name: sample\ninterview:\n  - id: catalog\n    type: document\n    prompt: Catalog\n    schema: {uri}\n    input: catalog.json\n"
        ),
    )
    .unwrap();
    folder
}

/// The threads of this process. Counted on Linux.
fn threads() -> Option<usize> {
    cfg!(target_os = "linux").then(|| fs::read_dir("/proc/self/task").unwrap().count())
}

type Loaded = Result<(), toha::LoadError>;

/// Start `Template::load(folder)` on a worker thread, under `signal` when
/// given, and return the receiver of its result and the thread.
fn start_load(
    folder: PathBuf,
    signal: Option<StopSignal>,
) -> (mpsc::Receiver<Loaded>, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let load = || toha::Template::load(&folder).map(|_| ());
        let result = match signal {
            Some(signal) => with_stop_signal(signal, load),
            None => load(),
        };
        sender.send(result).unwrap();
    });
    (receiver, worker)
}

#[test]
#[ignore = "subprocess helper for the schema stop scenarios"]
fn schema_worker() {
    let Ok(scenario) = std::env::var("TOHA_SCHEMA_SCENARIO") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("TOHA_SCHEMA_ROOT").unwrap());
    let uri = std::env::var("TOHA_SCHEMA_URI").unwrap();
    let folder = template(&root, &uri);
    let held = || root.join(HELD).exists();
    let closed = || root.join(CLIENT_CLOSED).exists();
    match scenario.as_str() {
        "stopped" => {
            let before = threads();
            let signal = StopSignal::new();
            let (receiver, worker) = start_load(folder, Some(signal.clone()));
            assert!(wait_for(Duration::from_secs(30), held), "never connected");
            // The endpoint holds the fetch: without the stop, the load waits.
            thread::sleep(Duration::from_millis(300));
            assert!(matches!(
                receiver.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            let stopped = Instant::now();
            signal.stop();
            let result = receiver
                .recv_timeout(BOUND)
                .expect("the load did not return within 2 s of the stop");
            let returned = stopped.elapsed();
            worker.join().expect("the calling thread returns");
            let error = result.expect_err("a stopped load fails");
            assert!(error.stopped, "{error}");
            assert!(
                error.to_string().contains("schema fetch was stopped"),
                "{error}"
            );
            // Observed before the endpoint is released.
            assert!(
                wait_for(BOUND.saturating_sub(stopped.elapsed()), closed),
                "the connection is still open {:?} after the stop",
                stopped.elapsed()
            );
            let closed_after = stopped.elapsed();
            assert!(!root.join(RELEASE).exists());
            if let (Some(before), Some(after)) = (before, threads()) {
                assert_eq!(after, before, "a thread outlived the stopped load");
            }
            println!(
                "observed: returned {returned:?} and connection closed {closed_after:?} after the stop signal"
            );
        }
        "stopped-before" => {
            let signal = StopSignal::new();
            signal.stop();
            let error = with_stop_signal(signal, || toha::Template::load(&folder))
                .expect_err("a stopped load fails");
            assert!(error.stopped, "{error}");
            thread::sleep(Duration::from_millis(500));
            assert!(
                !root.join(ACCEPTED).exists(),
                "the stopped load connected to the endpoint"
            );
            println!("observed: no connection was made");
        }
        "unstopped" | "never-fires" => {
            let signal = (scenario == "never-fires").then(StopSignal::new);
            let (receiver, worker) = start_load(folder, signal);
            assert!(wait_for(Duration::from_secs(30), held), "never connected");
            // Longer than the stop bound and curl's progress interval: no
            // timeout and no abort end the fetch.
            thread::sleep(Duration::from_millis(2500));
            assert!(
                matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)),
                "the load ended while the endpoint held it"
            );
            assert!(!closed());
            fs::write(root.join(RELEASE), "").unwrap();
            let result = receiver
                .recv_timeout(Duration::from_secs(30))
                .expect("the load returns once the endpoint closes");
            worker.join().unwrap();
            let error = result.expect_err("the closed endpoint fails the load");
            assert!(!error.stopped, "{error}");
            println!("observed: waited until the endpoint closed, then failed unstopped");
        }
        other => panic!("unknown scenario {other}"),
    }
}
