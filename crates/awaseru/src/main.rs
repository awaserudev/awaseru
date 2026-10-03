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
                        output and any panic (default: beside --home)

    --regions           list what the backend exposes and stop
    --state-digest      print one line a machine can compare, and nothing else
    -h, --help          this
";

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
            list_only,
            digest_only,
        }) => match session::run(&plan) {
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
        },
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
        list_only: bool,
        digest_only: bool,
    },
}

/// Hand-rolled, because an argument parser would be a dependency and this is
/// nine options (§17.2).
///
/// Every option takes its value as a separate word. `--frames=3` is not
/// accepted, and saying so is better than accepting one spelling and silently
/// ignoring the other.
fn parse(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut shared = PathBuf::from("awaseru.toml");
    let mut local = PathBuf::from("awaseru.local.toml");
    let mut home = std::env::temp_dir().join("awaseru-backend-home");
    let mut bound = None;
    let mut address = None;
    let mut within = None;
    let mut region = None;
    let mut offset = 0usize;
    let mut length = 256usize;
    let mut list_only = false;
    let mut digest_only = false;
    let mut anchor = None;
    let mut cache = std::env::temp_dir().join("awaseru-anchor-cache");

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
            "--home" => home = PathBuf::from(value()?),
            "--frames" => bound = Some(Bound::Frames(number(&value()?, 10)?)),
            "--instructions" => bound = Some(Bound::Instructions(number(&value()?, 10)?)),
            "--address" => address = Some(number(&value()?, 16)?),
            "--within" => within = Some(number(&value()?, 10)?),
            "--region" => region = Some(value()?),
            "--offset" => offset = number(&value()?, 10)? as usize,
            "--length" => length = number(&value()?, 10)? as usize,
            "--anchor" => anchor = Some(value()?),
            "--cache" => cache = PathBuf::from(value()?),
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
        let places = Box::new(awaseru::child::Where {
            shared,
            local,
            home: home.clone(),
            cache,
            // Beside the backend's own home by default, because that is where
            // the emulator's files already are and §6.1 says paths are
            // machine-local.
            log: log.unwrap_or_else(|| home.join("reference.log")),
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

    Ok(Command::Run {
        plan: Box::new(Plan {
            shared,
            local,
            home,
            bound: bound.unwrap_or(Bound::Frames(1)),
            anchor,
            cache,
            region,
            offset,
            length,
        }),
        list_only,
        digest_only,
    })
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

    fn parse_args(words: &[&str]) -> Result<Command, String> {
        parse(words.iter().map(|s| s.to_string()))
    }

    fn plan_of(words: &[&str]) -> Plan {
        match parse_args(words).expect("it parses") {
            Command::Run { plan, .. } => *plan,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn digest_only_of(words: &[&str]) -> bool {
        match parse_args(words).expect("it parses") {
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
        match parse_args(&["reference", "--home", "/tmp/elsewhere"]).expect("it parses") {
            Command::Reference { places } => {
                assert_eq!(places.log, PathBuf::from("/tmp/elsewhere/reference.log"));
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
    fn with_no_arguments_everything_has_a_default() {
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

    /// An anchor replaces the bound rather than adding to it, because an
    /// anchor carries its own definition (§4.7).
    #[test]
    fn an_anchor_can_be_asked_for_by_name() {
        let plan = plan_of(&["--anchor", "accepts-input"]);
        assert_eq!(plan.anchor.as_deref(), Some("accepts-input"));
        assert_eq!(
            plan.bound,
            Bound::Frames(1),
            "the bound keeps its default and is ignored, rather than being made to mean \
             something next to an anchor"
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
