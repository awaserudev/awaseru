//! §3.6's open question, measured — snapshots by value or by handle.
//!
//! > A snapshot of a console's full state is of the order of hundreds of
//! > kilobytes. Passing it by value across the API for every comparison is
//! > wasteful; a handle the tool holds is cheaper but makes the client's state
//! > harder to inspect. *What would settle it*: the first real client, and the
//! > measured cost of a routine-level cycle.
//!
//! The client exists now, so the cost can be taken rather than guessed. The two
//! routes are both in the protocol as built, and the names map onto it like
//! this:
//!
//! - **by handle** — `examine`: the tool holds the states, compares them, and
//!   only a verdict crosses. No bytes of either side's state are on the wire.
//! - **by value** — `read`: the bytes themselves cross, and the client can look
//!   at them, diff them, print them, keep them.
//!
//! Four costs are taken, in one process each where that matters: a cycle in
//! process, a cycle over the wire, a cycle plus the output span read back, and a
//! whole region moved by value. A fifth separates the transport from the
//! measurement: the framing alone, for a payload the size of a region.
//!
//! # What is asserted, and what is only printed
//!
//! Timings are printed. The assertions are the ones that cannot flake: that a
//! whole-region read brings back exactly the region's bytes, and that framing
//! that many bytes costs a small fraction of one cycle — a claim with three
//! orders of magnitude of margin, which is the point of taking the measurement
//! at all.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command as Spawn, Stdio};
use std::time::{Duration, Instant};

use awaseru::binding::Binding;
use awaseru::cache::Cache;
use awaseru::config::AnchorPolicy;
use awaseru::frame::{self, Frame};
use awaseru::protocol::{self, Command, Reply, PROTOCOL};
use awaseru_core::anchor::Anchors;
use awaseru_core::snapshot::Provenance;
use awaseru_core::Platform;
use awaseru_snes::fixture::{self, expected};
use awaseru_snes::{Reference, Startup};

/// Enough repetitions to see past one slow first run, and few enough to keep the
/// suite quick. The numbers that matter differ by orders of magnitude, so this
/// is not a benchmark and does not pretend to be one.
const ROUNDS: u32 = 5;
const BUDGET: u64 = 20_000;

fn input() -> Vec<u8> {
    (0..expected::ROUTINE_LENGTH)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
        .collect()
}

fn span(offset: usize) -> protocol::Span {
    protocol::Span {
        region: "work-ram".into(),
        offset,
        length: expected::ROUTINE_LENGTH,
    }
}

/// One cycle. `wrong` decides whether the candidate differs, which is what
/// decides whether §5.4's localisation has anything to do — asked for over a
/// candidate that agrees, it costs nothing, and a measurement that did not know
/// that would be recording a replay it never made.
fn cycle(localise: bool, wrong: bool) -> (Command, Vec<u8>) {
    let mut payload = input();
    payload.extend(if wrong {
        expected::routine_without_the_chain(&input())
    } else {
        expected::routine(&input())
    });
    (
        Command::Examine {
            routine: protocol::Routine {
                name: "running-total".into(),
                entry: expected::ROUTINE_ENTRY,
                returns_to: expected::ROUTINE_RETURN,
                within: BUDGET,
                reaching: None,
                from: None,
            },
            given: vec![span(expected::ROUTINE_INPUT_AT)],
            produced: vec![span(expected::ROUTINE_OUTPUT_AT)],
            control: None,
            coverage: None,
            localise,
        },
        payload,
    )
}

fn configure(dir: &Path, rom: &Path, library: &Path) {
    let digest = awaseru::digest::of_file(rom).expect("the image hashes");
    std::fs::write(
        dir.join("awaseru.toml"),
        format!(
            "[project]\nplatform = \"snes\"\n\n\
             [rom]\nsha256 = \"{digest}\"\n\n\
             [[emulator]]\nname = \"ref-a\"\nplatform = \"snes\"\nbackend = \"mesence\"\n\
             version = \"2.2.1\"\n\n\
             [reference]\nuse = \"ref-a\"\n\n\
             [anchors]\nverify_from_origin = 0\nreverify_at_end = false\n"
        ),
    )
    .expect("the shared half");
    std::fs::write(
        dir.join("awaseru.local.toml"),
        format!(
            "[rom]\npath = \"{}\"\n\n[emulator.ref-a]\npath = \"{}\"\n",
            rom.display(),
            library.display()
        ),
    )
    .expect("the machine-local half");
}

struct Client {
    server: std::process::Child,
    to: Option<ChildStdin>,
    from: BufReader<std::process::ChildStdout>,
}

impl Client {
    fn spawn(exe: &Path, dir: &Path) -> Self {
        let mut server = Spawn::new(exe)
            .arg("serve")
            .arg("--config")
            .arg(dir.join("awaseru.toml"))
            .arg("--local")
            .arg(dir.join("awaseru.local.toml"))
            .arg("--home")
            .arg(dir.join("home"))
            .arg("--cache")
            .arg(dir.join("cache"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the server starts");
        let to = server.stdin.take();
        let from = BufReader::new(server.stdout.take().expect("its output"));
        Client { server, to, from }
    }

    fn ask(&mut self, command: &Command, payload: &[u8]) -> (Reply, Vec<u8>) {
        let envelope = serde_json::to_string(command).expect("our own command");
        frame::write_frame(
            self.to.as_mut().expect("the pipe"),
            &Frame::with_payload(envelope, payload.to_vec()),
        )
        .expect("it takes the frame");
        let frame = frame::read_frame(&mut self.from)
            .expect("a frame comes back")
            .expect("the server is still there");
        let reply: Reply = serde_json::from_str(&frame.envelope).expect("our vocabulary");
        (reply, frame.payload)
    }

    fn finish(mut self) {
        self.to = None;
        let _ = self.server.wait();
    }
}

fn each(total: Duration) -> Duration {
    total / ROUNDS
}

#[test]
fn what_a_routine_level_cycle_costs_by_value_and_by_handle() {
    let Some(library) = std::env::var_os("AWASERU_TEST_BACKEND").map(PathBuf::from) else {
        eprintln!("SKIPPED: set AWASERU_TEST_BACKEND to a built backend library");
        return;
    };
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_awaseru"));

    let dir = std::env::temp_dir().join("awaseru-cost");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("a directory");
    let rom = dir.join("routine.sfc");
    std::fs::write(&rom, fixture::image_with_a_routine()).expect("write the image");
    configure(&dir, &rom, &library);

    // ---- the framing alone, for a payload the size of a region ------------
    // Taken first and without a backend, so that the transport's own cost is
    // known before anything is attributed to it.
    let region_sized = vec![0xA5u8; 128 * 1024];
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let mut bytes = Vec::new();
        frame::write_frame(
            &mut bytes,
            &Frame::with_payload("{\"result\":\"bytes\"}", region_sized.clone()),
        )
        .expect("it writes");
        let read = frame::read_frame(&mut std::io::Cursor::new(bytes))
            .expect("it reads")
            .expect("a frame");
        assert_eq!(read.payload.len(), region_sized.len());
    }
    let framing = each(began.elapsed());

    // ---- a cycle in process ----------------------------------------------
    let mut reference =
        Reference::open_with(&library, &dir.join("home"), &rom, Startup::default())
            .expect("it comes up");
    reference.set_watchdog(Duration::from_secs(30));
    let provenance = Provenance {
        reference: "fixture".into(),
        backend: "mesence".into(),
        version: reference.version().reported,
        software: "the routine fixture".into(),
    };
    let anchors = Anchors::new(vec![]).expect("none needed");
    let cache = Cache::at(dir.join("cache-in-process"));
    let mut binding = Binding::new(
        &mut reference,
        &anchors,
        &cache,
        provenance,
        AnchorPolicy {
            verify_from_origin: 0,
            reverify_at_end: false,
        },
    );
    assert!(
        !binding
            .apply(
                Command::Hello {
                    protocol: PROTOCOL,
                    client: "the cost test".into(),
                },
                &[],
            )
            .was_refused()
    );

    let (command, payload) = cycle(false, false);
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let answered = binding.apply(command.clone(), &payload);
        assert!(!answered.was_refused(), "{:?}", answered.reply);
    }
    let in_process = each(began.elapsed());

    // And with §5.4's localisation, over a candidate that **differs** — asked
    // for over one that agrees there is nothing to localise and no replay
    // happens, which the first version of this measurement did not notice: it
    // came out faster than the plain cycle, which is what gave it away.
    let (localising, payload_for_localising) = cycle(true, true);
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let answered = binding.apply(localising.clone(), &payload_for_localising);
        assert!(!answered.was_refused(), "{:?}", answered.reply);
    }
    let in_process_localising = each(began.elapsed());

    // The in-process reference is finished with before the server starts its
    // own, which keeps the two emulators in two processes rather than one.
    drop(binding);
    drop(reference);

    // ---- a cycle over the wire -------------------------------------------
    let mut client = Client::spawn(&exe, &dir);
    let (reply, _) = client.ask(
        &Command::Hello {
            protocol: PROTOCOL,
            client: "the cost test".into(),
        },
        &[],
    );
    assert!(matches!(reply, Reply::Hello { .. }), "{reply:?}");

    let began = Instant::now();
    for _ in 0..ROUNDS {
        let (reply, _) = client.ask(&command, &payload);
        assert!(matches!(reply, Reply::Report { .. }), "{reply:?}");
    }
    let over_the_wire = each(began.elapsed());

    // ---- a cycle, plus the output span brought back by value -------------
    let read_span = Command::Read {
        region: "work-ram".into(),
        offset: expected::ROUTINE_OUTPUT_AT,
        length: expected::ROUTINE_LENGTH,
    };
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let (reply, _) = client.ask(&command, &payload);
        assert!(matches!(reply, Reply::Report { .. }), "{reply:?}");
        let (reply, bytes) = client.ask(&read_span, &[]);
        assert!(matches!(reply, Reply::Bytes { .. }), "{reply:?}");
        assert_eq!(bytes.len(), expected::ROUTINE_LENGTH);
    }
    let with_the_span = each(began.elapsed());

    // ---- a whole region, by value ----------------------------------------
    // §3.6's "hundreds of kilobytes" made concrete: the console's work memory,
    // moved across the boundary, which is what a snapshot by value would do for
    // every comparison.
    let (reply, regions) = client.ask(&Command::Regions, &[]);
    let size = match &reply {
        Reply::Regions { regions } => regions
            .iter()
            .find(|r| r.name == "work-ram")
            .expect("it is there")
            .size,
        other => panic!("got {other:?}"),
    };
    assert!(regions.is_empty(), "a region list carries no payload");

    let read_region = Command::Read {
        region: "work-ram".into(),
        offset: 0,
        length: size,
    };
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let (reply, bytes) = client.ask(&read_region, &[]);
        assert!(matches!(reply, Reply::Bytes { .. }), "{reply:?}");
        assert_eq!(
            bytes.len(),
            size,
            "a whole region by value is the whole region"
        );
    }
    let whole_region = each(began.elapsed());

    client.finish();

    // ---- what it all cost -------------------------------------------------
    eprintln!("per routine-level cycle, {ROUNDS} rounds each:");
    eprintln!("  in process, verdict only       {in_process:?}");
    eprintln!("  in process, with §5.4's replay {in_process_localising:?}");
    eprintln!("  over the wire, verdict only    {over_the_wire:?}");
    eprintln!("  over the wire, + the span      {with_the_span:?}");
    eprintln!("  over the wire, a whole region  {whole_region:?} ({size} bytes)");
    eprintln!("  framing {} KiB, no backend     {framing:?}", region_sized.len() / 1024);

    // ---- the assertion that cannot flake ----------------------------------
    // Framing a region's worth of bytes against measuring one routine: the
    // margin is orders of magnitude, which is the answer to §3.6 and the reason
    // a by-value route is affordable at all.
    assert!(
        framing * 10 < in_process,
        "the transport is supposed to be the cheap part: framing {framing:?} against a cycle \
         of {in_process:?}"
    );
}
