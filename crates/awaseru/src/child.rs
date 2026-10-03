//! The reference in a child process — both halves of that conversation.
//!
//! # Why the reference is not in the server
//!
//! The backend writes to **standard output**, from C++, beneath Rust's capture:
//! 130 lines in a single routine-level run, measured (`doc/protocol.md`). §8.2
//! puts the client's protocol on standard input and output, and a
//! length-prefixed stream does not survive text injected into the middle of it.
//!
//! So the emulator gets a process of its own, where its voice is its own
//! problem, and the server keeps a clean pair of streams for the client. The
//! route not taken — duplicating the server's standard output with `dup2` and
//! pointing the original somewhere harmless — would have cost **no new
//! dependency**, because `libc` is already in the lock file through
//! `cpufeatures`. It needs `unsafe` outside the backend's `ffi` module, which
//! §17.1 forbids, and this shape is more robust anyway: a child can crash, hang
//! or be killed without taking the protocol down with it, and two children is
//! what §5.5's cross-check would eventually need (§13's Q10).
//!
//! # How the two talk
//!
//! Internal, and deliberately so: §8.2's public contract is the *client's*
//! stdin and stdout, and nothing here changes it.
//!
//! | | |
//! |---|---|
//! | commands | the child's **standard input** — measured: nothing in the library reads it |
//! | answers | the child's **standard error** — measured: the library never writes there |
//! | the emulator's voice | the child's **standard output**, redirected to a log file |
//! | a panic in the child | a hook that writes the panic to the log, so it cannot look like an answer |
//!
//! Both directions use §8.3's framing, so a child that says something
//! unparseable is a refusal rather than a wrong answer — which matters most for
//! the one thing a panic hook cannot catch, a library that starts writing to
//! standard error in some future version.
//!
//! # What a death looks like, and why it is never a verdict
//!
//! Measured in U1: the answer channel gives end of file, `wait` gives either an
//! exit code or a signal, and writing to a corpse gives `BrokenPipe` without
//! killing the parent. All three are reported as the child having died, with
//! whichever of the two `wait` gave. **A dead child is never an answer**: a
//! comparison whose reference stopped existing half way through has no verdict,
//! and §2.3's third value is for exactly this.

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Spawn, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::binding::{Answered, Binding};
use crate::frame::{self, Frame, FrameError};
use crate::protocol::{Command, Reply};

/// Why a conversation with the child did not happen.
#[derive(Debug)]
pub enum ChildError {
    /// The process could not be started at all.
    Spawn { exe: PathBuf, why: String },
    /// It stopped existing. **Not a verdict** — a measurement whose reference
    /// died has no answer, and saying it agreed or differed would be inventing
    /// one.
    Died { how: String, log: PathBuf },
    /// The framing could not be read or written.
    Frame(FrameError),
    /// It is still running and has stopped answering. **A third case, not a
    /// death**: a process that is alive and silent is a hang, and reporting it
    /// as gone would be a different claim from the true one.
    ///
    /// Here because a mutation turned out to hang rather than fail: with the
    /// answers written to the stream the emulator owns, the parent waited for
    /// ever. A tool whose failure mode is "no output" is worse than one that
    /// says what it waited for.
    Silent { waited: Duration, log: PathBuf },
    /// The child answered something that is not this vocabulary.
    NotOurs { envelope: String, why: String },
}

impl std::fmt::Display for ChildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChildError::Spawn { exe, why } => write!(
                f,
                "the reference process could not be started from {}: {why}",
                exe.display()
            ),
            ChildError::Died { how, log } => write!(
                f,
                "the reference process is gone ({how}), so this has no answer rather than a \
                 wrong one. What it said on its own output is in {}",
                log.display()
            ),
            ChildError::Silent { waited, log } => write!(
                f,
                "the reference process is still running and has not answered in {waited:?}, so \
                 this has no answer rather than a wrong one. What it said on its own output is \
                 in {}",
                log.display()
            ),
            ChildError::Frame(e) => write!(f, "{e}"),
            ChildError::NotOurs { envelope, why } => write!(
                f,
                "the reference process answered something this version does not understand: \
                 {why}. It said: {envelope}"
            ),
        }
    }
}

impl std::error::Error for ChildError {}

impl From<FrameError> for ChildError {
    fn from(e: FrameError) -> Self {
        ChildError::Frame(e)
    }
}

/// Where the child keeps its files.
#[derive(Debug, Clone)]
pub struct Where {
    pub shared: PathBuf,
    pub local: PathBuf,
    pub home: PathBuf,
    pub cache: PathBuf,
    /// Where the emulator's own output goes, and where a panic goes.
    pub log: PathBuf,
}

/// The reference, from the parent's side.
pub struct Child {
    process: std::process::Child,
    /// Held from the moment of spawning, because `wait` drops it — U1's trap,
    /// and the difference between `BrokenPipe` and a panic about a handle that
    /// is gone.
    to: Option<std::process::ChildStdin>,
    /// Frames, read by a thread of their own.
    ///
    /// A thread rather than a blocking read, for one reason: **a read on a pipe
    /// cannot be given a deadline**, and a parent that waits for ever on a
    /// child that is alive and silent is a tool with no failure mode at all.
    /// The channel turns that into `ChildError::Silent` after the watchdog.
    answers: Option<Receiver<Result<Frame, FrameError>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    log: PathBuf,
    watchdog: Duration,
}

/// How long a parent waits for one answer before saying the child is silent.
///
/// Generous, because an `examine` replays a routine several times and a cold
/// anchor replays from the origin. It is a bound rather than a tuning: §4.2's
/// "no unbounded run" applied to waiting for someone else's run.
pub const WATCHDOG: Duration = Duration::from_secs(120);

impl Child {
    /// Starts one, with its standard output pointed at the log.
    pub fn spawn(exe: &Path, places: &Where) -> Result<Self, ChildError> {
        if let Some(parent) = places.log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let log = std::fs::File::create(&places.log).map_err(|e| ChildError::Spawn {
            exe: exe.to_path_buf(),
            why: format!("the log at {} could not be opened: {e}", places.log.display()),
        })?;

        let mut process = Spawn::new(exe)
            .arg("reference")
            .arg("--config")
            .arg(&places.shared)
            .arg("--local")
            .arg(&places.local)
            .arg("--home")
            .arg(&places.home)
            .arg("--cache")
            .arg(&places.cache)
            .stdin(Stdio::piped())
            // The emulator's voice, and nothing of ours.
            .stdout(Stdio::from(log))
            // The answers.
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ChildError::Spawn {
                exe: exe.to_path_buf(),
                why: e.to_string(),
            })?;

        let to = process.stdin.take();
        let (sender, answers) = std::sync::mpsc::channel();
        let reader = process.stderr.take().map(|stderr| {
            std::thread::spawn(move || {
                let mut from = BufReader::new(stderr);
                loop {
                    match frame::read_frame(&mut from) {
                        // End of file: the child is finished. Dropping the
                        // sender is what tells the parent, which is also what
                        // happens if this thread dies.
                        Ok(None) => return,
                        Ok(Some(frame)) => {
                            if sender.send(Ok(frame)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = sender.send(Err(e));
                            return;
                        }
                    }
                }
            })
        });

        Ok(Child {
            process,
            to,
            answers: Some(answers),
            reader,
            log: places.log.clone(),
            watchdog: WATCHDOG,
        })
    }

    /// How long to wait for one answer. A test that wants a quick failure sets
    /// it short; nothing else should need to.
    pub fn set_watchdog(&mut self, waited: Duration) {
        self.watchdog = waited;
    }

    /// Where the emulator's output and any panic went.
    pub fn log(&self) -> &Path {
        &self.log
    }

    /// One command, one answer.
    pub fn ask(&mut self, command: &Command, payload: &[u8]) -> Result<Answered, ChildError> {
        let envelope = serde_json::to_string(command).map_err(|e| ChildError::NotOurs {
            envelope: format!("{command:?}"),
            why: format!("this command could not be written as JSON: {e}"),
        })?;

        // A write that fails is almost always a child that is gone — and that is
        // **not** the end of the matter, which is what a flaky test taught.
        //
        // The child may have spoken first: one that cannot open its reference
        // answers a framed refusal and leaves, so its answer can be sitting in
        // the channel while this write hits a closed pipe. Returning a death
        // here threw that refusal away and reported "the reference is gone"
        // where the child had said *why* it was gone — intermittently, because
        // it depends on whether the child was scheduled to exit before the
        // write landed. So a failed write falls through to the read below: a
        // channel with an answer in it answers, and a disconnected one is the
        // death.
        match self.to.as_mut() {
            None => {}
            Some(to) => {
                if let Err(e) = frame::write_frame(to, &Frame::with_payload(envelope, payload.to_vec()))
                    && !matches!(e, FrameError::Io(_))
                {
                    return Err(ChildError::Frame(e));
                }
            }
        }

        let watchdog = self.watchdog;
        let received = match self.answers.as_ref() {
            None => return Err(self.died()),
            Some(answers) => answers.recv_timeout(watchdog),
        };
        match received {
            // The sender is gone: end of file, or the reading thread stopped.
            // Either way there is no answer, and `wait` says which.
            //
            // **Now the only path for a death**, which it was not when this was
            // written: a failed write used to report one from above, and it
            // reported it over the top of an answer the child had already sent.
            // Everything that ends a child ends here instead.
            Err(RecvTimeoutError::Disconnected) => Err(self.died()),
            Err(RecvTimeoutError::Timeout) => Err(ChildError::Silent {
                waited: watchdog,
                log: self.log.clone(),
            }),
            Ok(Err(FrameError::Io(_))) | Ok(Err(FrameError::Truncated { .. })) => Err(self.died()),
            Ok(Err(other)) => Err(ChildError::Frame(other)),
            Ok(Ok(frame)) => {
                let reply: Reply =
                    serde_json::from_str(&frame.envelope).map_err(|e| ChildError::NotOurs {
                        envelope: frame.envelope.clone(),
                        why: e.to_string(),
                    })?;
                Ok(Answered {
                    reply,
                    payload: frame.payload,
                })
            }
        }
    }

    /// What `wait` says about a child that is no longer answering.
    fn died(&mut self) -> ChildError {
        let how = match self.process.try_wait() {
            Ok(Some(status)) => describe(&status),
            // Still running and not answering: that is a hang rather than a
            // death, and saying "it is gone" would be wrong. It is killed first
            // so the report is true by the time it is read.
            Ok(None) => {
                let _ = self.process.kill();
                let status = self.process.wait().ok();
                match status {
                    Some(status) => format!(
                        "it stopped answering while still running, so it was killed: {}",
                        describe(&status)
                    ),
                    None => "it stopped answering and could not be waited for".to_string(),
                }
            }
            Err(e) => format!("its status could not be read: {e}"),
        };
        ChildError::Died {
            how,
            log: self.log.clone(),
        }
    }

    /// Ends it, and reaps it.
    pub fn kill(&mut self) {
        // Dropping the pipe first so the child sees end of file and can leave
        // on its own; then the signal, for one that will not.
        self.to = None;
        let _ = self.process.kill();
        let _ = self.process.wait();
        // The reading thread ends when the stream does, which the kill
        // guarantees. Joined so that a killed child leaves nothing behind.
        self.answers = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.kill();
    }
}

fn describe(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("killed by signal {signal}");
        }
    }
    match status.code() {
        // 101 is what a Rust panic exits with, and the panic itself is in the
        // log rather than in the answer channel — which is the whole point of
        // the hook.
        Some(101) => "it panicked; the panic is in its log".to_string(),
        Some(code) => format!("it exited with status {code}"),
        None => "it ended for a reason this platform does not name".to_string(),
    }
}

// ----------------------------------------------------------- the child side --

/// The child's whole life: open the reference, answer frames until the parent
/// stops asking.
///
/// Commands arrive on standard input and answers leave on standard error, for
/// the reasons in this module's header. Standard output is left alone — the
/// emulator owns it.
pub fn attend(places: &Where) -> std::process::ExitCode {
    install_panic_hook(&places.log);

    let loaded = match crate::config::load(&places.shared, &places.local) {
        Ok(loaded) => loaded,
        Err(e) => return refuse_and_leave("a configuration this tool can load", &e.to_string()),
    };
    let mut reference = match crate::session::open_reference(&loaded, &places.home) {
        Ok(reference) => reference,
        Err(e) => return refuse_and_leave("a reference this tool can open", &e.to_string()),
    };

    let (emulator, _) = loaded.reference();
    let provenance = awaseru_core::snapshot::Provenance {
        reference: emulator.name.clone(),
        backend: emulator.backend.clone(),
        version: reference.version().reported,
        software: loaded.software.display().to_string(),
    };
    if let Err(e) = std::fs::create_dir_all(&places.cache) {
        return refuse_and_leave(
            "somewhere to keep the anchor cache",
            &format!("{}: {e}", places.cache.display()),
        );
    }
    let cache = crate::cache::Cache::at(&places.cache);
    let mut binding = Binding::new(
        &mut *reference,
        &loaded.anchors,
        &cache,
        provenance,
        loaded.configuration.anchors.clone(),
    );

    let mut input = std::io::stdin().lock();
    loop {
        let frame = match frame::read_frame(&mut input) {
            // The parent closed the pipe: it has finished with this reference,
            // which is not a failure.
            Ok(None) => return std::process::ExitCode::SUCCESS,
            Ok(Some(frame)) => frame,
            Err(e) => {
                return refuse_and_leave("a frame this tool can read", &e.to_string());
            }
        };

        let answered = match serde_json::from_str::<Command>(&frame.envelope) {
            Ok(command) => binding.apply(command, &frame.payload),
            Err(e) => Answered {
                reply: Reply::Refused {
                    looking_for: "a command this vocabulary has".to_string(),
                    found: format!("{}, which does not parse: {e}", frame.envelope),
                },
                payload: Vec::new(),
            },
        };

        if answer(&answered).is_err() {
            // The parent is gone. Nothing to report it to.
            return std::process::ExitCode::FAILURE;
        }
    }
}

/// Writes one answer on the channel the parent reads.
fn answer(answered: &Answered) -> Result<(), FrameError> {
    let envelope = serde_json::to_string(&answered.reply).map_err(|e| {
        FrameError::Io(std::io::Error::other(format!(
            "a reply that cannot be written as JSON: {e}"
        )))
    })?;
    let mut out = std::io::stderr().lock();
    frame::write_frame(
        &mut out,
        &Frame::with_payload(envelope, answered.payload.clone()),
    )
}

/// Says why it cannot start, **in a frame**, and leaves.
///
/// A plain message on standard error would be bytes in the middle of the answer
/// channel — exactly what the panic hook exists to prevent — so even dying is
/// framed.
fn refuse_and_leave(looking_for: &str, found: &str) -> std::process::ExitCode {
    let _ = answer(&Answered {
        reply: Reply::Refused {
            looking_for: looking_for.to_string(),
            found: found.to_string(),
        },
        payload: Vec::new(),
    });
    std::process::ExitCode::FAILURE
}

/// Sends panics to the log instead of to standard error.
///
/// Measured in U1: without this, a panic puts 213 bytes of English into the
/// answer channel, starting `thread '...' panicked at`. With it, the channel
/// carries answers and the panic is still visible twice — in the log, and in
/// the exit status.
fn install_panic_hook(log: &Path) {
    let log = log.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(file, "the reference process panicked: {info}");
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A death is described by what `wait` gave, and the three cases read
    /// differently — because "it panicked" and "it exited with status 3" are
    /// different things to go and do.
    #[test]
    fn a_death_says_which_kind_it_was() {
        // Built by running real processes: an exit status cannot be constructed
        // by hand, and a test that faked one would be testing the fake.
        let of = |args: &[&str]| {
            Spawn::new("/bin/sh")
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("sh runs")
        };

        let clean = describe(&of(&["-c", "exit 0"]));
        let refused = describe(&of(&["-c", "exit 3"]));
        let panicked = describe(&of(&["-c", "exit 101"]));
        let killed = describe(&of(&["-c", "kill -9 $$"]));

        assert!(clean.contains("status 0"), "{clean}");
        assert!(refused.contains("status 3"), "{refused}");
        assert!(panicked.contains("panicked"), "{panicked}");
        assert!(killed.contains("signal 9"), "{killed}");

        let all = [&clean, &refused, &panicked, &killed];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "two kinds of death read the same");
            }
        }
    }

    /// The three ways a question goes unanswered must read differently, and the
    /// middle one is the one this module gained after a mutation hung instead
    /// of failing: alive and silent is not the same as gone.
    #[test]
    fn silence_and_death_are_different_answers() {
        let silent = ChildError::Silent {
            waited: Duration::from_secs(120),
            log: PathBuf::from("/tmp/somewhere.log"),
        };
        let dead = ChildError::Died {
            how: "killed by signal 9".into(),
            log: PathBuf::from("/tmp/somewhere.log"),
        };

        let said = silent.to_string();
        assert!(said.contains("still running"), "{said}");
        assert!(said.contains("120s"), "it says what it waited: {said}");
        assert!(
            said.contains("no answer rather than a wrong one"),
            "neither is a verdict: {said}"
        );
        assert_ne!(said, dead.to_string());
        assert!(
            !said.contains("is gone"),
            "a hang must not be reported as a death: {said}"
        );
    }

    /// A refusal from a child that never started is still a refusal, and it
    /// names the log — because what the emulator said on its way down is the
    /// thing the reader needs next.
    #[test]
    fn a_dead_child_is_reported_as_dead_and_points_at_its_log() {
        let error = ChildError::Died {
            how: "killed by signal 9".into(),
            log: PathBuf::from("/tmp/somewhere.log"),
        };
        let said = error.to_string();
        assert!(said.contains("signal 9"), "{said}");
        assert!(said.contains("/tmp/somewhere.log"), "{said}");
        assert!(
            said.contains("no answer rather than a wrong one"),
            "a death must not read like a verdict: {said}"
        );
    }
}
