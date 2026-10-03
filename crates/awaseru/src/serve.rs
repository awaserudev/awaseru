//! §8.2's transport: the client's standard input and output, and nothing else.
//!
//! > Messages over the child process's standard input and output. No ports, no
//! > listening sockets, no firewall or permission dialog, and identical
//! > behaviour on every platform the host runs on.
//!
//! The client spawns `awaseru serve` (§8.1 — the client drives), writes framed
//! commands to its standard input and reads framed replies from its standard
//! output. The reference itself lives in a further child (`child.rs`), because
//! the emulator writes to standard output and a length-prefixed stream does not
//! survive that.
//!
//! # The server decides nothing
//!
//! It decodes a frame, hands the command to the reference process, and encodes
//! the answer. The handshake, the version check, every refusal and every verdict
//! are the binding's (§8.4), running in the child. That is deliberate: the
//! moment the server starts answering something itself, the two bindings have a
//! difference, and the difference will be found by a client rather than by a
//! test.
//!
//! What the server does own is the transport's own failures — a frame it cannot
//! read, a reference process that will not start, one that dies — and each of
//! those is a reply rather than a silence.
//!
//! # One reference per server, and it is never replaced
//!
//! §13's Q10: this backend's emulator is one object per process, so a server has
//! one reference. There is no command in the vocabulary that asks for a second,
//! and that is structural rather than a rule being enforced.
//!
//! **A reference that dies is not respawned.** A fresh process would be a fresh
//! machine at a fresh position, and handing that to a client in the middle of a
//! conversation would be handing it a different machine that looks like the one
//! it was using — the silent weakening this whole project is written against. So
//! the death is reported, and every command after it gets the same answer, until
//! the client closes the stream and starts again deliberately.

use std::io::Write;
use std::process::ExitCode;

use crate::binding::Answered;
use crate::child::{Child, ChildError, Where};
use crate::frame::{self, Frame, FrameError};
use crate::protocol::{Command, Reply};

/// Runs the server until the client closes its standard input.
pub fn serve(places: &Where) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            // Nothing can be done without it: the reference process is this
            // same binary. There is no frame to answer because no frame has
            // arrived yet, so this one goes to standard error.
            eprintln!("awaseru: this program cannot find its own path, so it cannot start a reference process: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut reference: Option<Child> = None;
    // Set once a reference has died. Kept so that every later command gets the
    // same answer rather than a confusing new one.
    let mut gone: Option<String> = None;

    loop {
        let frame = match frame::read_frame(&mut input) {
            // The client has finished. That is how a conversation ends.
            Ok(None) => return ExitCode::SUCCESS,
            Ok(Some(frame)) => frame,
            Err(e) => {
                // A frame that cannot be read cannot be answered in order:
                // the stream's position is unknown, so there is nothing to do
                // but say so and stop.
                let _ = say(
                    &mut output,
                    &Answered {
                        reply: Reply::Refused {
                            looking_for: "a frame this tool can read".to_string(),
                            found: e.to_string(),
                        },
                        payload: Vec::new(),
                    },
                );
                return ExitCode::FAILURE;
            }
        };

        let answered = match serde_json::from_str::<Command>(&frame.envelope) {
            Err(e) => Answered {
                reply: Reply::Refused {
                    looking_for: "a command this vocabulary has".to_string(),
                    found: format!("{}, which does not parse: {e}", frame.envelope),
                },
                payload: Vec::new(),
            },
            Ok(command) => {
                if let Some(how) = &gone {
                    refused_because(how)
                } else {
                    // The reference process starts with the first command, so
                    // that a client which only wants to say hello and leave
                    // does not pay for an emulator.
                    if reference.is_none() {
                        match Child::spawn(&exe, places) {
                            Ok(child) => reference = Some(child),
                            Err(e) => {
                                let answered = Answered {
                                    reply: Reply::Refused {
                                        looking_for: "a reference process".to_string(),
                                        found: e.to_string(),
                                    },
                                    payload: Vec::new(),
                                };
                                if say(&mut output, &answered).is_err() {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            }
                        }
                    }
                    let child = reference.as_mut().expect("just spawned");
                    match child.ask(&command, &frame.payload) {
                        Ok(answered) => answered,
                        Err(e) => {
                            // A reference that is gone or silent is remembered,
                            // because the next command must not look like it
                            // reached a machine.
                            if matches!(e, ChildError::Died { .. } | ChildError::Silent { .. }) {
                                gone = Some(e.to_string());
                            }
                            refused_because(&e.to_string())
                        }
                    }
                }
            }
        };

        if say(&mut output, &answered).is_err() {
            // The client is gone. Nothing to report it to.
            return ExitCode::FAILURE;
        }
    }
}

/// The answer when there is no reference to ask.
///
/// A refusal and never a verdict: §2.3's three values exist so that "I could not
/// look" has somewhere to go, and a reference that is not there is the clearest
/// case of it there is.
fn refused_because(why: &str) -> Answered {
    Answered {
        reply: Reply::Refused {
            looking_for: "an answer from the reference".to_string(),
            found: why.to_string(),
        },
        payload: Vec::new(),
    }
}

fn say(output: &mut impl Write, answered: &Answered) -> Result<(), FrameError> {
    let envelope = serde_json::to_string(&answered.reply).map_err(|e| {
        FrameError::Io(std::io::Error::other(format!(
            "a reply that cannot be written as JSON: {e}"
        )))
    })?;
    frame::write_frame(
        output,
        &Frame::with_payload(envelope, answered.payload.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two answers the server gives on its own behalf must both be
    /// refusals, and both must say what was looked for and what was found — a
    /// client cannot tell a transport failure from a verdict otherwise.
    #[test]
    fn what_the_server_answers_for_itself_is_always_a_refusal() {
        let answered = refused_because("the reference process is gone (killed by signal 9)");
        match &answered.reply {
            Reply::Refused { looking_for, found } => {
                assert!(looking_for.contains("reference"), "{looking_for}");
                assert!(found.contains("signal 9"), "{found}");
            }
            other => panic!("a missing reference is never a verdict: {other:?}"),
        }
        assert!(answered.payload.is_empty());
    }

    /// A reply goes out as one frame, and the frame is what the client reads.
    /// Checked here rather than only through a process, so that a change to the
    /// framing is caught by a fast test as well as a slow one.
    #[test]
    fn an_answer_goes_out_as_one_frame() {
        let mut out: Vec<u8> = Vec::new();
        say(
            &mut out,
            &Answered {
                reply: Reply::Bytes {
                    region: "work".into(),
                    offset: 0,
                    length: 3,
                },
                payload: vec![1, 2, 3],
            },
        )
        .expect("it writes");

        let frame = frame::read_frame(&mut std::io::Cursor::new(out))
            .expect("it reads")
            .expect("one frame");
        assert_eq!(frame.payload, vec![1, 2, 3]);
        let reply: Reply = serde_json::from_str(&frame.envelope).expect("our vocabulary");
        assert!(matches!(reply, Reply::Bytes { length: 3, .. }), "{reply:?}");
    }
}
