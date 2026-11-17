// ---
// relationships:
//   implements: architecture
// ---
//! A stop signal ends a template load that a stalled HTTPS document schema
//! endpoint holds: the load returns within 2 s and its connection is closed
//! before the endpoint lets go, and no thread outlives it. Without a signal,
//! or with one that never fires, the load waits for the endpoint as before.
//!
//! On Linux, a `getaddrinfo` interposer the test builds holds the schema
//! host's name lookup: the stopped load returns within 2 s without
//! connecting, and its detached resolver threads end once the lookup is
//! released (wyrd-company/toha#81). The same holds for the target lookup
//! curl runs itself behind a `socks4://` or `socks5://` proxy, whose proxy
//! connection closes at the stop.
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
/// Written by the interposer when it holds the schema host's lookup.
const LOOKUP_HELD: &str = "lookup-held";
/// Written by the worker to let the interposer answer the held lookup.
const LOOKUP_RELEASE: &str = "lookup-release";
/// A host name the interposer holds until [`LOOKUP_RELEASE`], then resolves
/// to 127.0.0.1.
const HELD_HOST: &str = "schema-held.invalid";
/// A host name the interposer resolves to 127.0.0.1 at once.
const RESOLVED_HOST: &str = "schema-resolved.invalid";
/// Written by the SOCKS proxy listener when it accepts the connection.
const PROXY_ACCEPTED: &str = "proxy-accepted";
/// Written by the SOCKS proxy listener when Toha closes that connection.
const PROXY_CLOSED: &str = "proxy-closed";
/// The `getaddrinfo` interposer, preloaded into the worker on Linux.
const INTERPOSER: &str = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef int (*lookup)(const char *, const char *, const struct addrinfo *,
                      struct addrinfo **);

static int real(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **res) {
  lookup next = (lookup)dlsym(RTLD_NEXT, "getaddrinfo");
  return next(node, service, hints, res);
}

static void marker(const char *name, char *path, size_t size) {
  snprintf(path, size, "%s/%s", getenv("TOHA_SCHEMA_ROOT"), name);
}

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **res) {
  char path[4096];
  if (node && strcmp(node, "schema-held.invalid") == 0) {
    marker("lookup-held", path, sizeof path);
    FILE *held = fopen(path, "w");
    if (held) fclose(held);
    marker("lookup-release", path, sizeof path);
    while (access(path, F_OK) != 0) usleep(10000);
    return real("127.0.0.1", service, hints, res);
  }
  if (node && strcmp(node, "schema-resolved.invalid") == 0)
    return real("127.0.0.1", service, hints, res);
  return real(node, service, hints, res);
}
"#;

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
    run_scenario_at(scenario, None);
}

/// [`run_scenario`], addressing the endpoint by `host` through the
/// [`INTERPOSER`] when given.
fn run_scenario_at(scenario: &str, host: Option<&str>) {
    run_scenario_with(scenario, host, None);
}

/// A SOCKS proxy on a local port that never relays. It writes
/// [`PROXY_ACCEPTED`] when it accepts, [`HELD`] once the client sends
/// anything, and [`PROXY_CLOSED`] when the client closes the connection. To a
/// SOCKS5 greeting it answers "no authentication", so that a libcurl that
/// resolves the target after the greeting gets that far; it never answers a
/// connect request.
fn silent_socks_proxy(root: &Path) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let root = root.to_owned();
    let proxy = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    fs::write(root.join(PROXY_ACCEPTED), "").unwrap();
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
        let mut greeted = false;
        while !root.join(RELEASE).exists() && started.elapsed() < Duration::from_secs(60) {
            let mut buffer = [0; 1024];
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    if buffer[0] == 5 && !greeted {
                        greeted = true;
                        stream.set_nonblocking(false).unwrap();
                        std::io::Write::write_all(&mut stream, &[5, 0]).unwrap();
                        stream.set_nonblocking(true).unwrap();
                    } else {
                        fs::write(root.join(HELD), "").unwrap();
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => break,
            }
            thread::sleep(Duration::from_millis(10));
        }
        if !root.join(RELEASE).exists() {
            fs::write(root.join(PROXY_CLOSED), "").unwrap();
        }
    });
    (port, proxy)
}

/// [`run_scenario_at`], with the worker's HTTPS traffic sent through a
/// [`silent_socks_proxy`] of `proxy` scheme when given.
fn run_scenario_with(scenario: &str, host: Option<&str>, proxy: Option<&str>) {
    let root = tempfile::tempdir().unwrap();
    let (mut uri, server) = stalled_endpoint(root.path());
    let mut worker = Command::new(std::env::current_exe().unwrap());
    support::isolate(&mut worker, root.path());
    if let Some(host) = host {
        uri = uri.replace("127.0.0.1", host);
        let source = root.path().join("interposer.c");
        let library = root.path().join("interposer.so");
        fs::write(&source, INTERPOSER).unwrap();
        let built = Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .arg("-ldl")
            .output()
            .expect("a C compiler builds the getaddrinfo interposer");
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        worker.env("LD_PRELOAD", &library);
    }
    worker
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env_remove("https_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("all_proxy")
        .env_remove("ALL_PROXY");
    let socks = proxy.map(|scheme| {
        let (port, thread) = silent_socks_proxy(root.path());
        let address = format!("{scheme}://127.0.0.1:{port}");
        worker
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .env("HTTPS_PROXY", &address)
            .env("https_proxy", &address);
        thread
    });
    let output = worker
        .args(["--ignored", "--exact", "schema_worker", "--nocapture"])
        .env_remove("GIT_SSL_CAINFO")
        .env("TOHA_SCHEMA_SCENARIO", scenario)
        .env("TOHA_SCHEMA_ROOT", root.path())
        .env("TOHA_SCHEMA_URI", &uri)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // A worker that failed may not have released the endpoint or a lookup.
    let _ = fs::write(root.path().join(RELEASE), "");
    let _ = fs::write(root.path().join(LOOKUP_RELEASE), "");
    server.join().unwrap();
    if let Some(socks) = socks {
        socks.join().unwrap();
    }
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

#[cfg(target_os = "linux")]
#[test]
fn a_stop_ends_a_template_load_whose_schema_lookup_is_held() {
    run_scenario_at("lookup-held", Some(HELD_HOST));
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_after_the_schema_lookup_leaves_no_thread() {
    run_scenario_at("stopped", Some(RESOLVED_HOST));
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_ends_a_load_whose_socks4_target_lookup_is_held() {
    run_scenario_with("socks-held", Some(HELD_HOST), Some("socks4"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_ends_a_load_whose_socks5_target_lookup_is_held() {
    run_scenario_with("socks-held", Some(HELD_HOST), Some("socks5"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_stop_behind_a_silent_socks_proxy_leaves_no_thread() {
    run_scenario_with("socks-silent", Some(RESOLVED_HOST), Some("socks4"));
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

/// The threads of this process named `toha-schema-release`. Counted on Linux.
fn release_threads() -> Option<usize> {
    cfg!(target_os = "linux").then(|| {
        fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|entry| fs::read_to_string(entry.ok()?.path().join("comm")).ok())
            .filter(|name| name.trim() == "toha-schema-release")
            .count()
    })
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
        "lookup-held" => {
            let accepted = || root.join(ACCEPTED).exists();
            let before = threads();
            let signal = StopSignal::new();
            let (receiver, worker) = start_load(folder, Some(signal.clone()));
            assert!(
                wait_for(Duration::from_secs(30), || root.join(LOOKUP_HELD).exists()),
                "the lookup never started"
            );
            // The lookup holds the fetch: without the stop, the load waits.
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
            // The documented exception: the lookup's resolver threads are
            // detached and run on until the system resolver returns. curl
            // reports this lookup as pending, so no Toha thread is among them.
            let during = threads();
            assert_eq!(
                release_threads().unwrap_or(0),
                0,
                "a release thread outlived a stop whose lookup curl reported as pending"
            );
            fs::write(root.join(LOOKUP_RELEASE), "").unwrap();
            let settled = || match (before, threads()) {
                (Some(before), Some(now)) => now == before,
                _ => true,
            };
            assert!(
                wait_for(Duration::from_secs(10), settled),
                "threads {:?} did not return to the baseline {before:?} after the lookup",
                threads()
            );
            // The resolved lookup does not reach the schema host.
            thread::sleep(Duration::from_millis(300));
            assert!(!accepted(), "the stopped load connected to the schema host");
            println!(
                "observed: returned {returned:?} after the stop signal while the lookup was held, \
                 never connected; threads {before:?} before, {during:?} while held, back to the baseline after the lookup returned"
            );
        }
        "socks-held" => {
            let before = threads();
            let signal = StopSignal::new();
            let (receiver, worker) = start_load(folder, Some(signal.clone()));
            assert!(
                wait_for(Duration::from_secs(30), || root.join(LOOKUP_HELD).exists()
                    && root.join(PROXY_ACCEPTED).exists()),
                "the proxy connection and the target lookup never started"
            );
            // The target lookup holds the fetch: without the stop, it waits.
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
            // Before the lookup is released, the proxy connection is closed.
            assert!(
                wait_for(BOUND.saturating_sub(stopped.elapsed()), || root
                    .join(PROXY_CLOSED)
                    .exists()),
                "the proxy connection is still open {:?} after the stop",
                stopped.elapsed()
            );
            let closed = stopped.elapsed();
            assert!(!root.join(LOOKUP_RELEASE).exists());
            // The documented exception: the release thread and the lookup's
            // resolver threads run on until the lookup returns.
            let during = threads();
            fs::write(root.join(LOOKUP_RELEASE), "").unwrap();
            let settled = || match (before, threads()) {
                (Some(before), Some(now)) => now == before,
                _ => true,
            };
            assert!(
                wait_for(Duration::from_secs(10), settled),
                "threads {:?} did not return to the baseline {before:?} after the lookup",
                threads()
            );
            println!(
                "observed: returned {returned:?} and proxy connection closed {closed:?} after the stop signal while the target lookup was held; \
                 threads {before:?} before, {during:?} while held, back to the baseline after the lookup returned"
            );
        }
        "socks-silent" => {
            let before = threads();
            let signal = StopSignal::new();
            let (receiver, worker) = start_load(folder, Some(signal.clone()));
            // The proxy has the connect request: the target lookup is done.
            assert!(
                wait_for(Duration::from_secs(30), held),
                "never reached the proxy"
            );
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
            assert!(result.expect_err("a stopped load fails").stopped);
            assert!(
                wait_for(BOUND.saturating_sub(stopped.elapsed()), || root
                    .join(PROXY_CLOSED)
                    .exists()),
                "the proxy connection is still open after the stop"
            );
            if let (Some(before), Some(after)) = (before, threads()) {
                assert_eq!(after, before, "a thread outlived the stopped load");
            }
            println!(
                "observed: returned {returned:?} after the stop signal behind a silent proxy; proxy connection closed; no thread left"
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
