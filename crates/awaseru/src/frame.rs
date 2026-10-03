//! §8.3's framing: a length-prefixed envelope and a length-prefixed payload.
//!
//! > A length-prefixed **JSON envelope** carrying the command or result,
//! > optionally followed by a length-prefixed **binary payload** carrying
//! > state.
//!
//! This module knows nothing about what either part means. It reads and writes
//! frames, refuses what it cannot read as a value (§14.2), and leaves the JSON
//! to the vocabulary above it. A framing layer that had to understand the
//! message to find its end could not skip a message it does not understand,
//! which is the first thing a protocol with a version needs to do.
//!
//! # The grammar, and the four decisions in it
//!
//! ```text
//! frame := envelope_len:u32be  envelope:bytes[envelope_len]
//!          payload_len:u32be   payload:bytes[payload_len]
//! ```
//!
//! **Four bytes, big-endian.** Network order, so a client in any language
//! writes it the obvious way — `struct.pack(">I", n)` in Python, which is the
//! second client §M4 asks for. Four bytes because the limits below are far
//! inside what it can express, and a width that could hold more than the reader
//! will accept would be a width that invites a refusal.
//!
//! **The payload's prefix is always there**, and zero means absent. §8.3 says
//! the payload is optional, and this is how it is optional: a reader that had to
//! consult the JSON to know whether more bytes followed could not find the end
//! of a message it could not parse — and finding the end of a message you do not
//! understand is exactly what a protocol version needs in order to refuse one
//! politely (§8.6). Four bytes of zero is the price, paid once per message.
//!
//! An empty payload and no payload are therefore the same thing. That costs
//! nothing: a payload carries state, and zero bytes of state is not a state.
//!
//! **An envelope of zero length is refused.** The shortest JSON value is two
//! bytes, so a zero-length envelope is not a message that happens to be empty;
//! it is a sender that has lost its place.
//!
//! **Both parts have a maximum, and the maximum is checked before anything is
//! allocated.** Nothing a client sends is trusted, a length least of all: a
//! four-byte number can ask for four gigabytes, and a reader that allocates
//! first and validates second is a reader any client can kill.
//!
//! | | limit | why |
//! |---|---|---|
//! | envelope | 1 MiB | the control plane is a command or a result; the largest this project can imagine is a region list, which is kilobytes |
//! | payload | 64 MiB | §8.3 puts a snapshot at "hundreds of kilobytes", and a console's whole memory is a couple of megabytes; 64 MiB leaves room for a batch without leaving room for an accident |

use std::io::{Read, Write};

/// The largest envelope a reader will accept.
pub const ENVELOPE_LIMIT: u32 = 1 << 20;

/// The largest payload a reader will accept.
pub const PAYLOAD_LIMIT: u32 = 64 << 20;

/// One message: its envelope, and the bytes that came with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The JSON text. Validated as UTF-8 and not as JSON — this layer does not
    /// know what a command is.
    pub envelope: String,
    /// The state that came with it, if any.
    pub payload: Vec<u8>,
}

impl Frame {
    /// An envelope on its own.
    pub fn of(envelope: impl Into<String>) -> Self {
        Frame {
            envelope: envelope.into(),
            payload: Vec::new(),
        }
    }

    /// An envelope with state behind it.
    pub fn with_payload(envelope: impl Into<String>, payload: Vec<u8>) -> Self {
        Frame {
            envelope: envelope.into(),
            payload,
        }
    }

    pub fn has_payload(&self) -> bool {
        !self.payload.is_empty()
    }
}

/// Why a frame could not be read or written — §14.2, every one of them a value.
#[derive(Debug)]
pub enum FrameError {
    /// The stream ended part way through a frame. Distinct from the clean end
    /// of a stream, which is not an error and is reported as `Ok(None)`: a
    /// sender that stopped between messages has finished, and one that stopped
    /// mid-message has died.
    Truncated {
        reading: &'static str,
        expected: usize,
        got: usize,
    },
    /// A length larger than this reader will accept. **Refused before
    /// allocating**, which is the whole point of having a limit.
    TooLarge {
        reading: &'static str,
        length: u32,
        limit: u32,
    },
    /// An envelope of no bytes. The shortest JSON value is two.
    EmptyEnvelope,
    /// The envelope is not UTF-8, so it is not JSON either.
    NotText { at: usize },
    /// The stream itself failed.
    Io(std::io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Truncated {
                reading,
                expected,
                got,
            } => write!(
                f,
                "the stream ended part way through the {reading}: {expected} byte(s) were \
                 promised and {got} arrived, so whoever was sending stopped mid-message"
            ),
            FrameError::TooLarge {
                reading,
                length,
                limit,
            } => write!(
                f,
                "the {reading} claims {length} bytes and this reader accepts at most {limit}. \
                 Nothing was allocated: a length is something the other side said, not something \
                 this side believes"
            ),
            FrameError::EmptyEnvelope => write!(
                f,
                "the envelope is zero bytes long, and the shortest JSON value is two — so this \
                 is not an empty message, it is a sender that has lost its place in the stream"
            ),
            FrameError::NotText { at } => write!(
                f,
                "the envelope is not valid UTF-8, from byte {at} on, so it cannot be JSON"
            ),
            FrameError::Io(e) => write!(f, "the stream failed: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        FrameError::Io(e)
    }
}

/// Reads one frame, or `None` at the clean end of the stream.
///
/// The difference between `Ok(None)` and `Err(Truncated)` is the difference
/// between a sender that finished and a sender that died, and §M4's child
/// processes are read with exactly that distinction in hand.
pub fn read_frame(reader: &mut impl Read) -> Result<Option<Frame>, FrameError> {
    let Some(envelope_len) = read_length(reader, "envelope")? else {
        return Ok(None);
    };
    if envelope_len == 0 {
        return Err(FrameError::EmptyEnvelope);
    }
    if envelope_len > ENVELOPE_LIMIT {
        return Err(FrameError::TooLarge {
            reading: "envelope",
            length: envelope_len,
            limit: ENVELOPE_LIMIT,
        });
    }
    let envelope = read_exactly(reader, envelope_len, "envelope")?;
    let envelope = String::from_utf8(envelope).map_err(|e| FrameError::NotText {
        at: e.utf8_error().valid_up_to(),
    })?;

    // The payload's prefix is not optional even when the payload is, so a
    // stream that ends here ended mid-frame.
    let Some(payload_len) = read_length(reader, "payload length")? else {
        return Err(FrameError::Truncated {
            reading: "payload length",
            expected: 4,
            got: 0,
        });
    };
    if payload_len > PAYLOAD_LIMIT {
        return Err(FrameError::TooLarge {
            reading: "payload",
            length: payload_len,
            limit: PAYLOAD_LIMIT,
        });
    }
    let payload = read_exactly(reader, payload_len, "payload")?;

    Ok(Some(Frame { envelope, payload }))
}

/// Writes one frame and flushes it.
///
/// Flushed here rather than left to the caller, because a frame sitting in a
/// buffer looks exactly like a peer that is thinking — and the caller who
/// forgets it is always the one waiting for the answer.
///
/// Built into one buffer and written once: a frame that reaches the other side
/// in pieces is still a frame, but a reader watching two writes interleave with
/// another thread's is not something to debug (§17.4 — the allocation is the
/// cheap part).
pub fn write_frame(writer: &mut impl Write, frame: &Frame) -> Result<(), FrameError> {
    let envelope = frame.envelope.as_bytes();
    let envelope_len = length_of(envelope.len(), "envelope", ENVELOPE_LIMIT)?;
    if envelope_len == 0 {
        return Err(FrameError::EmptyEnvelope);
    }
    let payload_len = length_of(frame.payload.len(), "payload", PAYLOAD_LIMIT)?;

    let mut out = Vec::with_capacity(8 + envelope.len() + frame.payload.len());
    out.extend_from_slice(&envelope_len.to_be_bytes());
    out.extend_from_slice(envelope);
    out.extend_from_slice(&payload_len.to_be_bytes());
    out.extend_from_slice(&frame.payload);

    writer.write_all(&out)?;
    writer.flush()?;
    Ok(())
}

/// A four-byte big-endian length, or `None` if the stream ended cleanly before
/// it began.
fn read_length(reader: &mut impl Read, reading: &'static str) -> Result<Option<u32>, FrameError> {
    let mut bytes = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match reader.read(&mut bytes[got..])? {
            0 => break,
            n => got += n,
        }
    }
    match got {
        0 => Ok(None),
        4 => Ok(Some(u32::from_be_bytes(bytes))),
        partial => Err(FrameError::Truncated {
            reading,
            expected: 4,
            got: partial,
        }),
    }
}

/// Reads exactly `len` bytes, having already checked `len` against a limit.
///
/// `take` rather than a buffer of `len` zeros, so that a length which passed
/// the limit but exceeds what is actually coming costs what arrives rather than
/// what was claimed.
fn read_exactly(
    reader: &mut impl Read,
    len: u32,
    reading: &'static str,
) -> Result<Vec<u8>, FrameError> {
    let len = len as usize;
    let mut buffer = Vec::new();
    let got = reader.take(len as u64).read_to_end(&mut buffer)?;
    if got != len {
        return Err(FrameError::Truncated {
            reading,
            expected: len,
            got,
        });
    }
    Ok(buffer)
}

fn length_of(len: usize, reading: &'static str, limit: u32) -> Result<u32, FrameError> {
    let as_u32 = u32::try_from(len).map_err(|_| FrameError::TooLarge {
        reading,
        length: u32::MAX,
        limit,
    })?;
    if as_u32 > limit {
        return Err(FrameError::TooLarge {
            reading,
            length: as_u32,
            limit,
        });
    }
    Ok(as_u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn encoded(frame: &Frame) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(&mut out, frame).expect("it writes");
        out
    }

    fn read_one(bytes: &[u8]) -> Result<Option<Frame>, FrameError> {
        read_frame(&mut Cursor::new(bytes.to_vec()))
    }

    #[test]
    fn a_frame_survives_the_round_trip_with_and_without_a_payload() {
        for frame in [
            Frame::of("{\"command\":\"regions\"}"),
            Frame::with_payload("{\"command\":\"seed\"}", vec![0, 1, 2, 250, 255]),
            // A payload of one zero byte is a payload: the length says one, and
            // "no payload" is zero.
            Frame::with_payload("{}", vec![0]),
        ] {
            let bytes = encoded(&frame);
            assert_eq!(
                read_one(&bytes).expect("it reads").expect("a frame"),
                frame
            );
        }
    }

    /// The grammar, byte for byte, because a second client is written against
    /// these numbers and not against this code.
    #[test]
    fn the_bytes_are_the_grammar_the_record_describes() {
        let bytes = encoded(&Frame::with_payload("{}", vec![0xAB, 0xCD]));
        assert_eq!(
            bytes,
            vec![
                0, 0, 0, 2, // envelope length, big-endian
                b'{', b'}', // envelope
                0, 0, 0, 2, // payload length, big-endian
                0xAB, 0xCD, // payload
            ]
        );

        // And without a payload the prefix is still there, holding zero.
        assert_eq!(
            encoded(&Frame::of("{}")),
            vec![0, 0, 0, 2, b'{', b'}', 0, 0, 0, 0]
        );
    }

    /// Two frames back to back are two frames, and nothing of the first leaks
    /// into the second. Without this, a reader that mis-measured a length would
    /// look right for one message and wrong forever after.
    #[test]
    fn frames_back_to_back_are_read_one_at_a_time() {
        let mut stream = encoded(&Frame::with_payload("{\"n\":1}", vec![1, 2, 3]));
        stream.extend(encoded(&Frame::of("{\"n\":2}")));
        stream.extend(encoded(&Frame::with_payload("{\"n\":3}", vec![9])));

        let mut cursor = Cursor::new(stream);
        let first = read_frame(&mut cursor).unwrap().expect("one");
        let second = read_frame(&mut cursor).unwrap().expect("two");
        let third = read_frame(&mut cursor).unwrap().expect("three");
        assert_eq!(first.envelope, "{\"n\":1}");
        assert_eq!(first.payload, vec![1, 2, 3]);
        assert_eq!(second.envelope, "{\"n\":2}");
        assert!(!second.has_payload());
        assert_eq!(third.payload, vec![9]);

        // And then the clean end of the stream, which is not an error.
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }

    /// The distinction §M4's child processes are read with: a sender that
    /// finished against a sender that died.
    #[test]
    fn a_clean_end_is_not_an_error_and_a_broken_one_is() {
        assert!(read_one(&[]).expect("clean end").is_none());

        let err = read_one(&[0, 0]).expect_err("half a length");
        assert!(
            matches!(
                err,
                FrameError::Truncated {
                    expected: 4,
                    got: 2,
                    ..
                }
            ),
            "got {err}"
        );
        assert!(err.to_string().contains("mid-message"), "said: {err}");

        // An envelope whose bytes never arrive.
        let err = read_one(&[0, 0, 0, 10, b'{']).expect_err("short envelope");
        assert!(
            matches!(
                err,
                FrameError::Truncated {
                    reading: "envelope",
                    expected: 10,
                    got: 1
                }
            ),
            "got {err}"
        );

        // A frame that ends where the payload length should be.
        let err = read_one(&[0, 0, 0, 2, b'{', b'}']).expect_err("no payload length");
        assert!(
            matches!(err, FrameError::Truncated { .. }),
            "a frame without its payload length is incomplete, not payload-free: {err}"
        );

        // A payload shorter than it claims.
        let err = read_one(&[0, 0, 0, 2, b'{', b'}', 0, 0, 0, 8, 1, 2, 3]).expect_err("short");
        assert!(
            matches!(
                err,
                FrameError::Truncated {
                    reading: "payload",
                    expected: 8,
                    got: 3
                }
            ),
            "got {err}"
        );
    }

    /// **The assertion this module exists for.** A four-byte number can ask for
    /// four gigabytes, and the refusal must arrive without anything being
    /// allocated — from four bytes of input, with nothing behind them.
    #[test]
    fn a_length_nobody_could_mean_is_refused_before_anything_is_allocated() {
        let err = read_one(&[0xFF, 0xFF, 0xFF, 0xFF]).expect_err("four gigabytes");
        match err {
            FrameError::TooLarge {
                reading: "envelope",
                length,
                limit,
            } => {
                assert_eq!(length, u32::MAX);
                assert_eq!(limit, ENVELOPE_LIMIT);
            }
            other => panic!("got {other}"),
        }
        assert!(
            read_one(&[0xFF, 0xFF, 0xFF, 0xFF])
                .expect_err("again")
                .to_string()
                .contains("Nothing was allocated"),
            "the refusal says so, because that is the property being claimed"
        );

        // The same for a payload, and here the envelope is real: a well-formed
        // message with an absurd payload is the likelier attack.
        let mut bytes = vec![0, 0, 0, 2, b'{', b'}'];
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        let err = read_one(&bytes).expect_err("four gigabytes of payload");
        assert!(
            matches!(
                err,
                FrameError::TooLarge {
                    reading: "payload",
                    ..
                }
            ),
            "got {err}"
        );
    }

    /// The boundary, both sides of it. A limit that refused what it allows, or
    /// allowed what it refuses, would be found by a client and not by a test.
    #[test]
    fn the_limit_accepts_what_it_allows_and_refuses_one_byte_more() {
        let at_limit = Frame::of("x".repeat(ENVELOPE_LIMIT as usize));
        let bytes = encoded(&at_limit);
        assert_eq!(
            read_one(&bytes).expect("exactly at the limit").expect("a frame"),
            at_limit
        );

        let over = Frame::of("x".repeat(ENVELOPE_LIMIT as usize + 1));
        let err = write_frame(&mut Vec::new(), &over).expect_err("one byte over");
        assert!(matches!(err, FrameError::TooLarge { .. }), "got {err}");

        // Read side: a claimed length one over, with the bytes to match, must
        // be refused rather than read. This is the half a missing check passes.
        let mut bytes = (ENVELOPE_LIMIT + 1).to_be_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(b'x', ENVELOPE_LIMIT as usize + 1));
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let err = read_one(&bytes).expect_err("one byte over, and all of it present");
        assert!(matches!(err, FrameError::TooLarge { .. }), "got {err}");
    }

    #[test]
    fn an_empty_envelope_is_a_sender_that_lost_its_place() {
        let err = read_one(&[0, 0, 0, 0]).expect_err("nothing in the envelope");
        assert!(matches!(err, FrameError::EmptyEnvelope), "got {err}");
        assert!(err.to_string().contains("lost its place"), "said: {err}");

        let err = write_frame(&mut Vec::new(), &Frame::of("")).expect_err("writing nothing");
        assert!(matches!(err, FrameError::EmptyEnvelope), "got {err}");
    }

    /// An envelope that is not UTF-8 is not JSON, and the refusal says where it
    /// stopped being text — which is what somebody debugging a client's encoder
    /// needs.
    #[test]
    fn an_envelope_that_is_not_text_is_refused_where_it_stops_being_text() {
        let mut bytes = vec![0, 0, 0, 4];
        bytes.extend_from_slice(&[b'{', 0xFF, 0xFE, b'}']);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let err = read_one(&bytes).expect_err("not text");
        match err {
            FrameError::NotText { at } => assert_eq!(at, 1, "the first byte that is not text"),
            other => panic!("got {other}"),
        }
    }

    /// A reader that only works when every byte arrives at once is a reader
    /// that works in tests and hangs on a pipe. This feeds it one byte at a
    /// time.
    #[test]
    fn a_frame_arriving_one_byte_at_a_time_is_still_one_frame() {
        struct Dribble(Vec<u8>, usize);
        impl Read for Dribble {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.1 >= self.0.len() || out.is_empty() {
                    return Ok(0);
                }
                out[0] = self.0[self.1];
                self.1 += 1;
                Ok(1)
            }
        }

        let frame = Frame::with_payload("{\"command\":\"run\"}", vec![7; 300]);
        let mut dribble = Dribble(encoded(&frame), 0);
        assert_eq!(
            read_frame(&mut dribble).expect("it reads").expect("a frame"),
            frame
        );
    }
}
