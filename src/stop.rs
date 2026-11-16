// ---
// relationships:
//   implements: architecture
// ---
//! A caller's signal that stops a template fetch or an apply.
//!
//! Run the operation inside [`with_stop_signal`] and call [`StopSignal::stop`]
//! from any thread. The operation returns the `Stopped` variant of its error
//! soon after, and writes no further file.
use std::{
    cell::RefCell,
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

/// A signal that stops the operations run inside [`with_stop_signal`].
///
/// Clones share one signal: [`stop`](Self::stop) on any clone stops every
/// operation that holds one. A signal cannot be reset; give each operation a
/// new one.
#[derive(Clone, Debug, Default)]
pub struct StopSignal(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    /// Read before each write, and by gix as its interrupt flag.
    stopped: AtomicBool,
    /// Wakes a caller that waits for a fetch running on a Toha thread.
    waiting: Mutex<()>,
    woken: Condvar,
}

impl StopSignal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop every operation that holds this signal. Callable from any thread,
    /// any number of times.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::SeqCst);
        // Taken so that a caller between its check and its wait cannot miss
        // the wake.
        let _held = self.lock();
        self.0.woken.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.0
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

thread_local! {
    static SIGNAL: RefCell<Option<StopSignal>> = const { RefCell::new(None) };
}

/// Run `operation` so that `signal` can stop it: template fetches and commit
/// lookups, the `git` question's remote probe, an apply and an update.
///
/// The signal applies on the calling thread until `operation` returns or
/// unwinds. Operations on other threads, and code after this call, keep the
/// behaviour without a signal. An inner call replaces the signal for its own
/// operation.
///
/// Once the signal fires, the operation returns the `Stopped` variant of its
/// error: [`SourceError`](crate::source::SourceError),
/// [`ResolveError`](crate::resolve::ResolveError),
/// [`CatalogError`](crate::catalog::CatalogError),
/// [`ApplyError`](crate::ApplyError) or
/// [`MergeError`](crate::applied::MergeError), carried by the errors that wrap
/// them. A probe reports [`RemoteState::Stopped`](crate::git_target::RemoteState).
///
/// While a signal is in scope, a fetch and a probe run on a thread Toha
/// starts, so that the calling thread can return while a remote holds the
/// connection without answering.
pub fn with_stop_signal<T>(signal: StopSignal, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<StopSignal>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            SIGNAL.with(|current| *current.borrow_mut() = previous);
        }
    }
    let _restore = Restore(SIGNAL.with(|current| current.replace(Some(signal))));
    operation()
}

/// The calling thread's operation was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stopped;

fn current() -> Option<StopSignal> {
    SIGNAL.with(|current| current.borrow().clone())
}

/// Whether the calling thread's operation has a signal.
pub(crate) fn scoped() -> bool {
    SIGNAL.with(|current| current.borrow().is_some())
}

/// Whether the calling thread's operation was stopped.
pub(crate) fn requested() -> bool {
    SIGNAL.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(StopSignal::is_stopped)
    })
}

/// `Err(Stopped)` once the calling thread's operation was stopped. Called
/// before each write.
pub(crate) fn check() -> Result<(), Stopped> {
    if requested() { Err(Stopped) } else { Ok(()) }
}

/// Call `operation` with gix's interrupt flag for the calling thread's
/// operation: the signal's, or one that never fires.
pub(crate) fn with_interrupt<T>(operation: impl FnOnce(&AtomicBool) -> T) -> T {
    match current() {
        Some(signal) => operation(&signal.0.stopped),
        None => operation(&AtomicBool::new(false)),
    }
}

/// Run `work`, which may block on a remote, so that the calling thread's
/// signal can end the wait.
///
/// Without a signal, `work` runs on the calling thread with an interrupt flag
/// that never fires, as before the signal existed. With one, `work` runs on a
/// new thread, under the calling thread's signal and credential resolver, with
/// the signal as its interrupt flag. The caller returns `Err(Stopped)` as
/// soon as the signal fires, even when the thread is blocked on a read; that
/// thread ends at gix's next interrupt check, when the remote sends or closes,
/// and drops what `work` returns. A panic in `work` resumes on the caller.
pub(crate) fn run<T: Send + 'static>(
    work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> Result<T, Stopped> {
    let Some(signal) = current() else {
        return Ok(work(&AtomicBool::new(false)));
    };
    if signal.is_stopped() {
        return Err(Stopped);
    }
    let resolver = crate::source::current_resolver();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = signal.clone();
    std::thread::Builder::new()
        .name("toha-remote".into())
        .spawn(move || {
            let scoped = || {
                with_stop_signal(worker.clone(), || {
                    panic::catch_unwind(AssertUnwindSafe(|| work(&worker.0.stopped)))
                })
            };
            let outcome = match resolver {
                Some(resolver) => crate::source::with_credential_resolver(resolver, scoped),
                None => scoped(),
            };
            // A caller that already returned has dropped the receiver; the
            // outcome, and any folder it owns, is dropped here.
            let _ = sender.send(outcome);
            drop(worker.lock());
            worker.0.woken.notify_all();
        })
        .expect("failed to spawn a Toha remote thread");
    let mut held = signal.lock();
    loop {
        // A result that arrives together with the signal is dropped: the
        // caller asked to stop.
        if signal.is_stopped() {
            return Err(Stopped);
        }
        match receiver.try_recv() {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(payload)) => {
                drop(held);
                panic::resume_unwind(payload)
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                unreachable!("the remote thread sends before it ends")
            }
        }
        held = signal
            .0
            .woken
            .wait(held)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn without_a_signal_work_runs_on_the_calling_thread() {
        let caller = std::thread::current().id();
        let ran = run(move |interrupt| {
            assert!(!interrupt.load(Ordering::SeqCst));
            std::thread::current().id()
        });
        assert_eq!(ran, Ok(caller));
        assert!(!requested());
        assert_eq!(check(), Ok(()));
    }

    #[test]
    fn a_signal_is_scoped_to_its_operation_and_thread() {
        let signal = StopSignal::new();
        signal.stop();
        let inner = StopSignal::new();
        with_stop_signal(signal.clone(), || {
            assert!(requested());
            assert_eq!(check(), Err(Stopped));
            with_stop_signal(inner.clone(), || assert!(!requested()));
            assert!(requested());
            assert!(!std::thread::spawn(requested).join().unwrap());
        });
        assert!(!requested());
    }

    #[test]
    fn work_runs_with_the_signal_on_another_thread() {
        let signal = StopSignal::new();
        let caller = std::thread::current().id();
        let observed = with_stop_signal(signal.clone(), || {
            run(move |interrupt| {
                (
                    std::thread::current().id(),
                    requested(),
                    interrupt.load(Ordering::SeqCst),
                    current().is_some(),
                )
            })
        });
        let (thread, stopped, interrupted, scoped) = observed.unwrap();
        assert_ne!(thread, caller);
        assert!(!stopped && !interrupted && scoped);
    }

    #[test]
    fn a_stop_ends_the_wait_for_blocked_work() {
        let signal = StopSignal::new();
        let (release, held) = mpsc::channel::<()>();
        let (ended, finished) = mpsc::channel();
        let stopper = signal.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            stopper.stop();
        });
        let started = Instant::now();
        let result = with_stop_signal(signal, || {
            run(move |interrupt| {
                // Blocked as a read on a stalled remote is: the flag does not
                // wake it.
                held.recv().ok();
                ended.send(interrupt.load(Ordering::SeqCst)).unwrap();
            })
        });
        assert_eq!(result, Err(Stopped));
        assert!(started.elapsed() < Duration::from_secs(5));
        release.send(()).unwrap();
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the work sees the interrupt flag once it is unblocked"
        );
    }

    #[test]
    fn a_stopped_signal_starts_no_work() {
        let signal = StopSignal::new();
        signal.stop();
        let result = with_stop_signal(signal, || run(|_| panic!("work started")));
        assert_eq!(result, Err(Stopped));
    }

    #[test]
    fn a_panic_in_the_work_resumes_on_the_caller() {
        let result = panic::catch_unwind(|| {
            with_stop_signal(StopSignal::new(), || run(|_| panic!("from the work")))
        });
        let payload = result.unwrap_err();
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"from the work"));
    }
}
