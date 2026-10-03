//! awaseru — the host.
//!
//! M0's walking skeleton as a program: read the configuration (§6), select a
//! backend by name (§7.1), drive the reference to a position (§4.2), and print
//! one region's bytes.
//!
//! The work is in `session`; this is arguments and output. Keeping them apart
//! is what lets the test that is M0's done-condition run the host rather than a
//! second copy of it.

use std::path::PathBuf;
use std::process::ExitCode;

use awaseru::session::{self, Plan};
use awaseru_core::Bound;

const USAGE: &str = "\
awaseru — runs a reference and reports what is in it.

    awaseru [options]
    awaseru serve [options]       speak §8's protocol on stdin and stdout
    awaseru reference [options]   the reference process, which a server spawns

Options
    --config PATH       the shared configuration        (default awaseru.toml)
    --local PATH        the machine-local configuration (default awaseru.local.toml)
    --session PATH      one named directory holding one piece of work: its
                        backend home, its anchor cache, its runs and its logs.
                        The last part of the path is the session's name. Nothing
                        is shared between two sessions, and nothing here looks
                        for another one's work
    --home PATH         where the backend may keep its own files

    --frames N          run to the end of N frames      (default 1)
    --instructions N    run for N instructions
    --address ADDR      run until the program counter reaches ADDR (hexadecimal)
    --within N          how many instructions an --address bound may spend
                        looking. Required with --address (§4.4)

    --anchor NAME       arrive at this anchor instead of running a bound (§4.7)
    --cache PATH        where the anchor cache lives (machine-local, §6.7)

    --region NAME       which region to read (default: the first the backend reports)
    --offset N          where in it to start             (default 0)
    --length N          how many bytes                   (default 256)

    --log PATH          where `awaseru reference` sends the emulator's own
                        output and any panic. The default is beside --home,
                        named awaseru-YYYYMMDD-HHMMSS-mmm.log, so one run never
                        erases the one before it

    --regions           list what the backend exposes and stop
    --state-digest      print one line a machine can compare, and nothing else
    -h, --help          this
";

/// The default name of the log a reference writes its emulator's output to.
///
/// `awaseru-20261003-124233-512.log`: the project's name, then the moment, down
/// to the millisecond.
///
/// # Why the moment is in the name
///
/// It was `reference.log`, one fixed name, so **every run erased the one
/// before it**. The emulator's output is the only record of what it said while
/// something went wrong, and the usual way to find out something went wrong is
/// to look afterwards — by which time a second run has already happened.
///
/// Kept rather than rotated or numbered, because these are for a person: to
/// read, to filter, to send to somebody else who might recognise what the
/// emulator was complaining about, and to compare against the same run on a
/// later version. A fixed prefix makes them easy to find and to sweep; the
/// moment makes them impossible to confuse.
///
/// The arithmetic below is the civil-from-days one, written out rather than
/// taken as a sixth dependency for twenty lines.
fn log_name() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (secs, millis) = (now.as_secs(), now.subsec_millis());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Days since 1970-01-01 to a civil date. Shifts the era to start in March
    // so that a leap day lands at the end of a year and the month lengths run
    // in a repeating pattern.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);

    format!("awaseru-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{millis:03}.log")
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Serve { places }) => awaseru::serve::serve(&places),
        Ok(Command::Reference { places }) => awaseru::child::attend(&places),
        Ok(Command::Run {
            plan,
            session: named,
            list_only,
            digest_only,
        }) => {
            // Opened here and held for the whole run: the lock is what makes
            // one session one piece of work, and dropping it early would let a
            // second process in halfway through.
            let held = match named {
                None => None,
                Some(dir) => match awaseru::workspace::Session::open(dir) {
                    Ok(session) => Some(session),
                    Err(e) => {
                        eprintln!("awaseru: {e}");
                        return ExitCode::FAILURE;
                    }
                },
            };
            let outcome = session::run_in(&plan, held.as_ref());
            drop(held);
            match outcome {
            Ok(outcome) => {
                if digest_only {
                    print!("{}", digest_line(&outcome));
                } else {
                    report(&outcome, list_only);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                // Refusals are the product (§2.4), so they go to stderr whole,
                // with the reason the configuration or the backend gave.
                eprintln!("awaseru: {e}");
                ExitCode::FAILURE
            }
            }
        }
        Err(e) => {
            eprintln!("awaseru: {e}\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// One line, for a machine to compare — §2.5's "the same state" across
/// processes.
///
/// Everything in it is deterministic. The durations are deliberately **not**,
/// because two runs that took different amounts of time are still the same run
/// and a comparison that included the clock would never agree.
fn digest_line(outcome: &session::Outcome) -> String {
    format!(
        "stop={} state={} how={} evidence={}\n",
        outcome.stop.position,
        outcome.state,
        outcome
            .arrived
            .as_ref()
            .map(|a| match a.how {
                awaseru::arrive::How::Replayed { .. } => "replayed",
                awaseru::arrive::How::Resumed => "resumed",
            })
            .unwrap_or("no-anchor"),
        match &outcome.arrived {
            Some(arrived) => {
                if arrived.is_evidence() {
                    "yes"
                } else {
                    "no"
                }
            }
            None => {
                if outcome.beginning.repeats() {
                    "yes"
                } else {
                    "no"
                }
            }
        }
    )
}

fn report(outcome: &session::Outcome, list_only: bool) {
    println!(
        "reference {} — backend {} {}{}",
        outcome.emulator,
        outcome.backend,
        outcome.version.reported,
        outcome
            .version
            .built
            .as_deref()
            .map(|b| format!(", built {b}"))
            .unwrap_or_default()
    );

    println!("\nregions the backend exposes:");
    for region in outcome.regions.iter() {
        println!(
            "    {:<14} {:>9} bytes  {}",
            region.name,
            region.size,
            match (region.access.readable(), region.access.writable()) {
                (true, true) => "read/write",
                (true, false) => "read-only",
                (false, true) => "write-only",
                (false, false) => "neither",
            }
        );
    }

    if list_only {
        return;
    }

    // §4.12, next to the result and not in a footnote.
    println!("\n{}", outcome.beginning);
    if let Some(arrived) = &outcome.arrived {
        println!("{arrived}");
        if !arrived.is_evidence() {
            println!(
                "  → a comparison from this run is NOT evidence; the line above says why"
            );
        }
    }

    println!("\nfrom {}", outcome.started);
    println!("  to {}", outcome.stop);
    println!(
        "\n{} — {} of {} bytes from {:#x}:",
        outcome.region.name,
        outcome.bytes.len(),
        outcome.region.size,
        outcome.offset
    );
    print!("{}", session::hexdump(&outcome.bytes, outcome.offset));
}

#[derive(Debug)]
enum Command {
    Help,
    /// The server — `awaseru serve`, which a client spawns (§8.1, §8.2).
    Serve { places: Box<awaseru::child::Where> },
    /// The reference process — `awaseru reference`, which the server spawns and
    /// nobody runs by hand.
    ///
    /// It is a subcommand of the same binary rather than a second one, because
    /// a client installs `awaseru` and the server has to be able to find it:
    /// `current_exe` is a path that always exists, and a sibling binary is a
    /// path that may not have been installed.
    Reference { places: Box<awaseru::child::Where> },
    Run {
        /// Boxed because a `Plan` carries a `Bound`, and a bound can name a
        /// byte by region (§5.4's localisation) — which makes it large enough
        /// that `Help` would be paying for it.
        plan: Box<Plan>,
        /// The session to open and hold for the run, if one was named. Held by
        /// `main` rather than resolved away here, because opening it takes a
        /// lock and parsing arguments must not take anything.
        session: Option<PathBuf>,
        list_only: bool,
        digest_only: bool,
    },
}

/// Hand-rolled, because an argument parser would be a dependency and §17.2
/// keeps those to what is decided in `doc/dependencies.md`.
///
/// Every option takes its value as a separate word. `--frames=3` is not
/// accepted, and saying so is better than accepting one spelling and silently
/// ignoring the other.
fn parse(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut shared = PathBuf::from("awaseru.toml");
    let mut local = PathBuf::from("awaseru.local.toml");
    // No default, because the one it had was a single fixed path under the
    // system's temporary directory that EVERY invocation on the machine wrote
    // into, whatever software or piece of work it belonged to. §6.7 refuses a
    // shared cache of blobs; that default obeyed its letter, since nobody had
    // configured anything, and broke the reason. A bad default is the one kind
    // of default worth taking away.
    let mut home: Option<PathBuf> = None;
    let mut bound = None;
    let mut address = None;
    let mut within = None;
    let mut region = None;
    let mut offset = 0usize;
    let mut length = 256usize;
    let mut list_only = false;
    let mut digest_only = false;
    let mut anchor = None;
    let mut cache: Option<PathBuf> = None;
    // The session: one named directory holding one piece of work. Additive —
    // `--home` and `--cache` do exactly what they did, and either still
    // overrides what a session would have supplied.
    let mut session: Option<PathBuf> = None;

    let mut log: Option<PathBuf> = None;
    let mut as_reference = false;
    let mut as_server = false;

    let mut args = args.peekable();
    // The one bare word this tool takes, and it has to be first: a subcommand
    // after the options would be ambiguous with an option's value.
    match args.peek().map(String::as_str) {
        Some("reference") => {
            as_reference = true;
            args.next();
        }
        Some("serve") => {
            as_server = true;
            args.next();
        }
        _ => {}
    }
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("`{arg}` needs a value after it"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--config" => shared = PathBuf::from(value()?),
            "--local" => local = PathBuf::from(value()?),
            "--home" => home = Some(PathBuf::from(value()?)),
            "--frames" => bound = Some(Bound::Frames(number(&value()?, 10)?)),
            "--instructions" => bound = Some(Bound::Instructions(number(&value()?, 10)?)),
            "--address" => address = Some(number(&value()?, 16)?),
            "--within" => within = Some(number(&value()?, 10)?),
            "--region" => region = Some(value()?),
            "--offset" => offset = number(&value()?, 10)? as usize,
            "--length" => length = number(&value()?, 10)? as usize,
            "--anchor" => anchor = Some(value()?),
            "--cache" => cache = Some(PathBuf::from(value()?)),
            "--session" => session = Some(PathBuf::from(value()?)),
            "--log" => log = Some(PathBuf::from(value()?)),
            "--regions" => list_only = true,
            "--state-digest" => digest_only = true,
            other if other.starts_with('-') => {
                return Err(format!("`{other}` is not an option awaseru has"));
            }
            other => return Err(format!("awaseru takes options, not `{other}`")),
        }
    }

    if as_reference || as_server {
        let found = where_things_go(session, home, cache, log)?;
        let places = Box::new(awaseru::child::Where {
            shared,
            local,
            home: found.home,
            cache: found.cache,
            log: found.log,
        });
        return Ok(if as_server {
            Command::Serve { places }
        } else {
            Command::Reference { places }
        });
    }

    // §4.4: an address is the first bound that can fail to arrive, so it
    // carries a budget. A default would be a number nobody chose.
    let bound = match (address, within) {
        (Some(address), Some(within)) => Some(Bound::Address { address, within }),
        (Some(_), None) => {
            return Err("`--address` needs `--within N`: every run carries a budget (§4.4), \
                        because a run that does not stop is indistinguishable from one that has \
                        not finished"
                .to_string());
        }
        (None, Some(_)) => {
            return Err("`--within` is the budget for `--address` and means nothing without \
                        one; a frame or instruction bound always arrives"
                .to_string());
        }
        (None, None) => bound,
    };

    // §2.4, applied to arguments: an anchor carries its own definition, so a
    // bound beside one is two instructions and the tool does not pick between
    // them. It used to discard the bound in silence, which made §4.8's fourth
    // step — run the same bound onward from a replay and from a resume —
    // look askable and do nothing.
    if let (Some(_), Some(bound)) = (&anchor, &bound) {
        return Err(format!(
            "`--anchor` carries its own definition (§4.7), so the {} beside it would be a \
             second instruction about how far to run. One or the other: drop the bound to \
             arrive at the anchor, or drop `--anchor` to run it",
            match bound {
                Bound::Frames(_) => "`--frames`",
                Bound::Instructions(_) => "`--instructions`",
                _ => "`--address`",
            }
        ));
    }

    // Resolved here and not before the bound: `--anchor x --frames 3` is two
    // instructions and refusing it is the more useful answer, so a missing
    // place must not get in front of it.
    let found = where_things_go(session.clone(), home, cache, log)?;

    Ok(Command::Run {
        plan: Box::new(Plan {
            shared,
            local,
            home: found.home,
            bound: bound.unwrap_or(Bound::Frames(1)),
            anchor,
            cache: found.cache,
            region,
            offset,
            length,
        }),
        session,
        list_only,
        digest_only,
    })
}

/// The three places a run writes to, and where they come from.
struct Found {
    home: PathBuf,
    cache: PathBuf,
    log: PathBuf,
}

/// Resolves where things go, from a session or from the paths themselves.
///
/// A session supplies all three; `--home` and `--cache` still override what it
/// would have supplied, so neither option changed and nothing was taken away
/// except a default that was wrong.
///
/// With no session and no paths this **refuses**. There is deliberately no
/// fallback: the one it had sent every invocation on the machine into two fixed
/// directories, which is the thing §6.7 is about, and a tool that guessed a
/// place for a blob would be guessing about the one artefact where being wrong
/// is invisible.
fn where_things_go(
    session: Option<PathBuf>,
    home: Option<PathBuf>,
    cache: Option<PathBuf>,
    log: Option<PathBuf>,
) -> Result<Found, String> {
    match session {
        Some(dir) => Ok(Found {
            home: home.unwrap_or_else(|| dir.join("home")),
            cache: cache.unwrap_or_else(|| dir.join("anchors")),
            // The NAME carries the moment, so that one run does not erase the
            // one before it — see `log_name`.
            log: log.unwrap_or_else(|| dir.join("logs").join(log_name())),
        }),
        None => {
            let (home, cache) = match (home, cache) {
                (Some(home), Some(cache)) => (home, cache),
                (home, cache) => {
                    let mut missing = Vec::new();
                    if home.is_none() {
                        missing.push("--home");
                    }
                    if cache.is_none() {
                        missing.push("--cache");
                    }
                    return Err(format!(
                        "there is nowhere for this run to keep anything: {} {} given. Either                          name a session with `--session PATH`, which supplies both, or give                          them. There is no default, because the default used to be one place                          per machine that every piece of work shared, and §6.7 says a blob is                          the one thing where a stale copy from somebody else's run is invisible",
                        missing.join(" and "),
                        if missing.len() == 1 { "was not" } else { "were not" }
                    ));
                }
            };
            Ok(Found {
                // Beside the backend's own home by default, because that is
                // where the emulator's files already are and §6.1 says paths
                // are machine-local.
                log: log.unwrap_or_else(|| home.join(log_name())),
                home,
                cache,
            })
        }
    }
}

fn number(text: &str, radix: u32) -> Result<u64, String> {
    let cleaned = text.trim_start_matches("0x").replace('_', "");
    u64::from_str_radix(&cleaned, radix).map_err(|_| {
        format!(
            "`{text}` is not a number in base {radix}",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log's name carries the moment, so one run does not erase the one
    /// before it — which the fixed name it replaced did, every time.
    #[test]
    fn a_log_is_named_for_the_moment_it_was_opened() {
        let name = log_name();
        assert!(name.starts_with("awaseru-"), "a fixed prefix to find and sweep: {name}");
        assert!(name.ends_with(".log"), "{name}");

        // awaseru-YYYYMMDD-HHMMSS-mmm.log
        let middle = name
            .trim_start_matches("awaseru-")
            .trim_end_matches(".log");
        let parts: Vec<&str> = middle.split('-').collect();
        assert_eq!(parts.len(), 3, "date, time, milliseconds: {name}");
        assert_eq!(parts[0].len(), 8, "YYYYMMDD: {name}");
        assert_eq!(parts[1].len(), 6, "HHMMSS: {name}");
        assert_eq!(parts[2].len(), 3, "milliseconds: {name}");
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())), "{name}");

        // The date arithmetic is written out rather than taken as a
        // dependency, so it is worth checking it produces a plausible one.
        let year: i64 = parts[0][..4].parse().expect("a year");
        let month: u32 = parts[0][4..6].parse().expect("a month");
        let day: u32 = parts[0][6..].parse().expect("a day");
        assert!((2020..2200).contains(&year), "{name}");
        assert!((1..=12).contains(&month), "{name}");
        assert!((1..=31).contains(&day), "{name}");

        let hour: u32 = parts[1][..2].parse().expect("an hour");
        let minute: u32 = parts[1][2..4].parse().expect("a minute");
        let second: u32 = parts[1][4..].parse().expect("a second");
        assert!(hour < 24 && minute < 60 && second < 60, "{name}");
    }

    fn parse_args(words: &[&str]) -> Result<Command, String> {
        parse(words.iter().map(|s| s.to_string()))
    }

    /// A place to keep things, for the tests that are about something else.
    ///
    /// Added here rather than in each test, because the tests below are about
    /// bounds, regions and anchors, and a session is what they all now need to
    /// have somewhere to put a blob.
    fn with_a_session(words: &[&str]) -> Vec<String> {
        let mut argv = vec!["--session".to_string(), "/tmp/awaseru-parse-test".to_string()];
        argv.extend(words.iter().map(|w| w.to_string()));
        argv
    }

    fn plan_of(words: &[&str]) -> Plan {
        match parse(with_a_session(words).into_iter()).expect("it parses") {
            Command::Run { plan, .. } => *plan,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn parse_of(words: &[&str]) -> Result<Command, String> {
        parse(with_a_session(words).into_iter())
    }

    fn digest_only_of(words: &[&str]) -> bool {
        match parse(with_a_session(words).into_iter()).expect("it parses") {
            Command::Run { digest_only, .. } => digest_only,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// The subcommand is the one bare word this tool takes, and it has to be
    /// first — after an option it would be ambiguous with that option's value.
    #[test]
    fn the_reference_subcommand_takes_the_same_paths_and_a_log() {
        match parse_args(&[
            "reference",
            "--config",
            "/tmp/shared.toml",
            "--local",
            "/tmp/local.toml",
            "--home",
            "/tmp/home",
            "--cache",
            "/tmp/cache",
            "--log",
            "/tmp/talk.log",
        ])
        .expect("it parses")
        {
            Command::Reference { places } => {
                assert_eq!(places.shared, PathBuf::from("/tmp/shared.toml"));
                assert_eq!(places.local, PathBuf::from("/tmp/local.toml"));
                assert_eq!(places.home, PathBuf::from("/tmp/home"));
                assert_eq!(places.cache, PathBuf::from("/tmp/cache"));
                assert_eq!(places.log, PathBuf::from("/tmp/talk.log"));
            }
            other => panic!("expected the reference process, got {other:?}"),
        }

        // The log has a default beside the backend's home, because the
        // emulator's files are already there (§6.1).
        // Both paths, because the parent always spawns the child with both
        // (`child::Child::spawn`) and there is no longer a shared default to
        // fall back to.
        match parse_args(&[
            "reference",
            "--home",
            "/tmp/elsewhere",
            "--cache",
            "/tmp/elsewhere-anchors",
        ])
        .expect("it parses")
        {
            Command::Reference { places } => {
                let log = places.log.display().to_string();
                assert!(
                    log.starts_with("/tmp/elsewhere/awaseru-") && log.ends_with(".log"),
                    "beside the home, and carrying the moment so one run does not erase the \
                     one before it: {log}"
                );
            }
            other => panic!("got {other:?}"),
        }

        // And the word only means a subcommand when it is first: anywhere else
        // it is refused rather than guessed at.
        assert!(parse_args(&["--home", "/tmp", "reference"]).is_err());
    }

    /// The defaults are the whole of M0's command line: no arguments at all
    /// must be a complete request.
    #[test]
    fn everything_except_a_place_to_keep_things_has_a_default() {
        let plan = plan_of(&[]);
        assert_eq!(plan.shared, PathBuf::from("awaseru.toml"));
        assert_eq!(plan.local, PathBuf::from("awaseru.local.toml"));
        assert_eq!(plan.bound, Bound::Frames(1));
        assert_eq!(plan.region, None, "which region is the backend's to say");
        assert_eq!((plan.offset, plan.length), (0, 256));
        assert_eq!(plan.anchor, None, "a bound unless an anchor is asked for");
        assert!(!digest_only_of(&[]), "the ordinary output is for a person");
        assert!(digest_only_of(&["--state-digest"]));
    }

    /// The default that was taken away, and why taking it away is the fix.
    ///
    /// It used to be two fixed paths under the system's temporary directory, so
    /// every invocation on the machine wrote into the same backend home and the
    /// same anchor cache whatever work it belonged to. §6.7 refuses a shared
    /// cache of blobs because a stale blob is invisible; the default obeyed its
    /// letter — nobody had configured anything — and broke its reason.
    #[test]
    fn with_nowhere_to_keep_anything_a_run_is_refused_and_says_both_ways_out() {
        let err = parse(["--frames", "3"].iter().map(|w| w.to_string()))
            .expect_err("nowhere to put a blob is not a thing to guess");
        assert!(err.contains("--session"), "said: {err}");
        assert!(err.contains("--home"), "said: {err}");
        assert!(err.contains("--cache"), "said: {err}");
        assert!(
            err.contains("every piece of work shared"),
            "the refusal has to say why there is no default, said: {err}"
        );

        // One of the two is still nowhere to keep the other.
        let err = parse(["--home", "/tmp/h"].iter().map(|w| w.to_string()))
            .expect_err("half a place is not a place");
        assert!(err.contains("--cache"), "said: {err}");
        assert!(!err.contains("--home was not"), "said: {err}");
    }

    /// A session supplies all three, and either path still overrides it — the
    /// option is additive and nothing was renamed.
    #[test]
    fn a_session_supplies_the_places_and_an_explicit_path_still_wins() {
        let plan = match parse(
            ["--session", "/tmp/work/ff5-battle"]
                .iter()
                .map(|w| w.to_string()),
        )
        .expect("it parses")
        {
            Command::Run { plan, session, .. } => {
                assert_eq!(session, Some(PathBuf::from("/tmp/work/ff5-battle")));
                *plan
            }
            other => panic!("expected a run, got {other:?}"),
        };
        assert_eq!(plan.home, PathBuf::from("/tmp/work/ff5-battle/home"));
        assert_eq!(plan.cache, PathBuf::from("/tmp/work/ff5-battle/anchors"));

        let plan = plan_of(&["--cache", "/somewhere-else"]);
        assert_eq!(
            plan.cache,
            PathBuf::from("/somewhere-else"),
            "an explicit path overrides what the session would have supplied"
        );
        assert_eq!(
            plan.home,
            PathBuf::from("/tmp/awaseru-parse-test/home"),
            "and the one not given still comes from the session"
        );
    }

    /// A bound beside an anchor is refused rather than discarded — §2.4.
    ///
    /// It used to be ignored in silence, which is how `--anchor X --frames 300`
    /// came to print the digest of X and say nothing. Everything else in this
    /// tool refuses rather than choosing between two instructions, and this is
    /// the one place that chose.
    #[test]
    fn a_bound_beside_an_anchor_is_refused_and_not_ignored() {
        for bound in [
            vec!["--frames", "300"],
            vec!["--instructions", "900"],
            vec!["--address", "C40000", "--within", "1000"],
        ] {
            let mut argv = vec!["--anchor", "booted"];
            argv.extend(bound.iter().copied());
            let err = parse_of(&argv).expect_err("two instructions, no guessing");
            assert!(
                err.contains("--anchor") && err.contains("§4.7"),
                "the refusal names both halves: {err}"
            );
        }

        // And the half that keeps it from being a blanket refusal: each still
        // works alone.
        assert_eq!(plan_of(&["--frames", "300"]).bound, Bound::Frames(300));
        assert_eq!(plan_of(&["--anchor", "booted"]).anchor.as_deref(), Some("booted"));
    }

    /// An anchor replaces the bound rather than adding to it, because an
    /// anchor carries its own definition (§4.7).
    #[test]
    fn an_anchor_can_be_asked_for_by_name() {
        let plan = plan_of(&["--anchor", "accepts-input"]);
        assert_eq!(plan.anchor.as_deref(), Some("accepts-input"));
        assert_eq!(
            plan.bound,
            Bound::Frames(1),
            "the bound keeps its default, which is what an anchor's own definition replaces"
        );
        assert!(
            plan.cache.to_string_lossy().contains("awaseru"),
            "the cache has a default place, because §6.7 keeps it out of configuration"
        );
        assert_eq!(
            plan_of(&["--cache", "/somewhere"]).cache,
            PathBuf::from("/somewhere")
        );
    }

    #[test]
    fn each_bound_can_be_asked_for() {
        assert_eq!(plan_of(&["--frames", "7"]).bound, Bound::Frames(7));
        assert_eq!(
            plan_of(&["--instructions", "1000"]).bound,
            Bound::Instructions(1000)
        );
        assert_eq!(
            plan_of(&["--address", "C40000", "--within", "90"]).bound,
            Bound::Address {
                address: 0xC4_0000,
                within: 90
            },
            "an address is hexadecimal, because that is how addresses are written"
        );
        assert_eq!(
            plan_of(&["--address", "0xC40000", "--within", "90"]).bound,
            Bound::Address {
                address: 0xC4_0000,
                within: 90
            },
            "and the prefix is accepted rather than refused on a technicality"
        );
    }

    /// §4.4 at the command line: an address without a budget is refused, and
    /// a budget without an address is too. A default budget would be a number
    /// nobody chose, which is the same argument that makes the bound itself
    /// mandatory for an anchor.
    #[test]
    fn an_address_at_the_command_line_needs_a_budget_and_the_reverse() {
        let err = parse_args(&["--address", "8000"]).expect_err("no budget");
        assert!(err.contains("--within"), "said: {err}");
        assert!(
            err.contains("has not finished"),
            "the message must say why an unbounded run is not a run: {err}"
        );

        let err = parse_args(&["--within", "90"]).expect_err("no address");
        assert!(err.contains("always arrives"), "said: {err}");
    }

    /// The last bound wins, rather than two being combined into something
    /// neither was asked for.
    #[test]
    fn the_last_bound_asked_for_is_the_one_used() {
        assert_eq!(
            plan_of(&["--frames", "3", "--instructions", "9"]).bound,
            Bound::Instructions(9)
        );
    }

    /// An option nobody has is an error, not a thing to ignore. Ignoring it
    /// means a typed `--framse 600` runs one frame and says nothing.
    #[test]
    fn an_unknown_option_is_refused_and_named() {
        let err = parse_args(&["--framse", "600"]).expect_err("no such option");
        assert!(err.contains("--framse"), "said: {err}");

        let err = parse_args(&["somefile"]).expect_err("not an option");
        assert!(err.contains("somefile"), "said: {err}");
    }

    #[test]
    fn an_option_with_no_value_is_refused() {
        let err = parse_args(&["--frames"]).expect_err("nothing after it");
        assert!(err.contains("--frames") && err.contains("value"), "said: {err}");
    }

    #[test]
    fn a_bound_that_is_not_a_number_is_refused() {
        let err = parse_args(&["--frames", "lots"]).expect_err("not a number");
        assert!(err.contains("lots"), "said: {err}");
        let err = parse_args(&["--address", "zz"]).expect_err("not hexadecimal");
        assert!(err.contains("base 16"), "said: {err}");
    }

    /// `--frames=3` is not a spelling awaseru takes, and it says so. Accepting
    /// one form and quietly dropping the other is how an option comes to be
    /// typed for months without working.
    #[test]
    fn an_option_joined_to_its_value_is_refused_rather_than_half_read() {
        let err = parse_args(&["--frames=3"]).expect_err("not a spelling we take");
        assert!(err.contains("--frames=3"), "said: {err}");
    }

    #[test]
    fn help_is_help_wherever_it_appears() {
        assert!(matches!(parse_args(&["--help"]), Ok(Command::Help)));
        assert!(matches!(
            parse_args(&["--frames", "2", "-h"]),
            Ok(Command::Help)
        ));
    }
}
