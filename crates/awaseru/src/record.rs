//! What was asked, when, and what came back.
//!
//! Until this module existed, **no answer this tool produced ever reached the
//! disk.** A run was a plan in, an outcome out, printed, gone. A person who
//! wanted to know what a run had said yesterday had their terminal's scrollback
//! and nothing else, and nothing could be handed to anybody.
//!
//! ## The shape on disk is decided here, not derived
//!
//! The file is written field by field rather than by deriving a serialiser over
//! the types in `awaseru-core`. Those types change as the tool learns things;
//! a format derived from them would change with them, and §8.6 is explicit that
//! a new field is additive while a new variant is not. So the on-disk answer is
//! a small fixed vocabulary, and adding to it is a decision somebody makes here
//! rather than a consequence of renaming an enum somewhere else.
//!
//! ## What the reader must never do
//!
//! §8.6 says a reader is tolerant. §2.3 says *not determined* is never
//! collapsed into *agrees*. Together they decide the one rule that matters
//! here: an answer this build does not recognise reads back as **not
//! determined**, with the word it did not recognise. A tolerant reader that
//! treated an unknown answer as agreement would turn a future version's verdict
//! into a pass, which is the single worst thing this file could do.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use awaseru_core::verdict::Verdict;

/// One answer, in the vocabulary that goes to disk.
///
/// `Agrees` is **only** ever written for something that was compared. An
/// arrival is not a comparison: it reached a position and read bytes, and
/// calling that agreement would be §2.3's collapse with extra steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A position was reached. Carries how, and the state digest, which is what
    /// §2.5 compares across processes.
    Arrived { how: String, state: String },
    Agrees { compared: usize, moved: usize },
    Differs { because: String },
    /// §2.3's third value, and what an unrecognised answer reads back as.
    NotDetermined { because: String },
}

impl Answer {
    /// The word that goes on disk.
    pub fn word(&self) -> &'static str {
        match self {
            Answer::Arrived { .. } => "arrived",
            Answer::Agrees { .. } => "agrees",
            Answer::Differs { .. } => "differs",
            Answer::NotDetermined { .. } => "not-determined",
        }
    }

    /// Whether this answer can be built on. Three values, and the two that are
    /// not agreement are not the same as each other.
    pub fn is_agreement(&self) -> bool {
        matches!(self, Answer::Agrees { .. })
    }
}

impl From<&Verdict> for Answer {
    fn from(verdict: &Verdict) -> Self {
        match verdict {
            Verdict::Agrees { compared, moved } => Answer::Agrees {
                compared: *compared,
                moved: *moved,
            },
            Verdict::Differs(difference) => Answer::Differs {
                because: difference.to_string(),
            },
            Verdict::NotDetermined(why) => Answer::NotDetermined {
                because: why.to_string(),
            },
        }
    }
}

/// One run, as it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The question, as arguments that would ask it again.
    ///
    /// Arguments rather than a parallel structure, because a structure would be
    /// a second description of a question that already has one and the two
    /// would drift. Machine-local paths are left out: §6.1 says where things
    /// are belongs to this machine, and they are not part of the question.
    pub asked: String,
    /// When, as `YYYYMMDD-HHMMSS-mmm`.
    pub at: String,
    /// What it was asked of. A received record is somebody else's measurement,
    /// and these four fields are how a reader can tell.
    pub software: String,
    pub reference: String,
    pub backend: String,
    pub version: String,
    /// How long the run took. Kept out of any comparison of two records: two
    /// runs that took different amounts of time are still the same run.
    pub took: Duration,
    pub answer: Answer,
}

#[derive(Debug)]
pub enum RecordError {
    Io { at: PathBuf, why: std::io::Error },
    Unreadable { at: PathBuf, why: &'static str },
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::Io { at, why } => {
                write!(f, "`{}` could not be used: {why}", at.display())
            }
            RecordError::Unreadable { at, why } => {
                write!(f, "`{}` is not a run this tool wrote: {why}", at.display())
            }
        }
    }
}

impl std::error::Error for RecordError {}

impl Record {
    /// Writes this into `runs`, numbered after whatever is already there.
    ///
    /// The name is `NNN-<verb>-<subject>.toml`, so that a directory listing is
    /// already a history in order. The number is the count of what is there
    /// plus one and is **not** a key: two records that collided on a name would
    /// be a file overwritten, which is why the name is made unique by the
    /// moment as well.
    pub fn keep(&self, runs: &Path, verb: &str, subject: &str) -> Result<PathBuf, RecordError> {
        std::fs::create_dir_all(runs).map_err(|why| RecordError::Io {
            at: runs.to_path_buf(),
            why,
        })?;
        let number = std::fs::read_dir(runs)
            .map(|entries| entries.flatten().count())
            .unwrap_or(0)
            + 1;
        let subject = sanitised(subject);
        let at = runs.join(format!(
            "{number:03}-{verb}-{subject}-{}.toml",
            self.at
        ));
        std::fs::write(&at, self.to_text()).map_err(|why| RecordError::Io {
            at: at.clone(),
            why,
        })?;
        Ok(at)
    }

    /// The file's contents. One flat table, so that a person can read it and a
    /// reader does not have to understand nesting to find the answer.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("# What was asked, and what came back. Written by awaseru.\n");
        out.push_str("#\n");
        out.push_str("# `answer` is one of agrees, differs, not-determined, arrived.\n");
        out.push_str("# An answer a reader does not know is NOT determined (§2.3): never\n");
        out.push_str("# agreement, whatever a later version may add here.\n");
        out.push_str(&format!("asked = {:?}\n", self.asked));
        out.push_str(&format!("at = {:?}\n", self.at));
        out.push_str(&format!("took_seconds = {:.3}\n", self.took.as_secs_f64()));
        out.push_str(&format!("software = {:?}\n", self.software));
        out.push_str(&format!("reference = {:?}\n", self.reference));
        out.push_str(&format!("backend = {:?}\n", self.backend));
        out.push_str(&format!("version = {:?}\n", self.version));
        out.push_str(&format!("answer = {:?}\n", self.answer.word()));
        match &self.answer {
            Answer::Arrived { how, state } => {
                out.push_str(&format!("how = {how:?}\n"));
                out.push_str(&format!("state = {state:?}\n"));
            }
            Answer::Agrees { compared, moved } => {
                out.push_str(&format!("compared = {compared}\n"));
                // §2.2: agreement over nothing is vacuous, so the number that
                // makes it a measurement rather than a decoration is kept with
                // it and not left to be inferred.
                out.push_str(&format!("moved = {moved}\n"));
            }
            Answer::Differs { because } | Answer::NotDetermined { because } => {
                out.push_str(&format!("because = {because:?}\n"));
            }
        }
        out
    }

    /// Reads one back.
    ///
    /// Tolerant about fields it does not know (§8.6) and **not** tolerant about
    /// the answer: a word this build does not have reads as not determined,
    /// carrying the word, because the alternative is turning a future
    /// version's verdict into a pass.
    pub fn read(at: &Path) -> Result<Record, RecordError> {
        let text = std::fs::read_to_string(at).map_err(|why| RecordError::Io {
            at: at.to_path_buf(),
            why,
        })?;
        Record::parse(&text).map_err(|why| RecordError::Unreadable {
            at: at.to_path_buf(),
            why,
        })
    }

    pub fn parse(text: &str) -> Result<Record, &'static str> {
        let field = |key: &str| -> Option<String> {
            text.lines()
                .filter_map(|line| line.split_once('='))
                .find(|(name, _)| name.trim() == key)
                .map(|(_, value)| value.trim().trim_matches('"').to_string())
        };
        let word = field("answer").ok_or("it says no answer at all")?;
        let because = field("because").unwrap_or_default();
        let answer = match word.as_str() {
            "arrived" => Answer::Arrived {
                how: field("how").unwrap_or_default(),
                state: field("state").unwrap_or_default(),
            },
            "agrees" => Answer::Agrees {
                compared: field("compared")
                    .and_then(|n| n.parse().ok())
                    .ok_or("it says it agrees and does not say over how many bytes")?,
                moved: field("moved")
                    .and_then(|n| n.parse().ok())
                    .ok_or("it says it agrees and does not say how much moved (§2.2)")?,
            },
            "differs" => Answer::Differs { because },
            "not-determined" => Answer::NotDetermined { because },
            other => Answer::NotDetermined {
                because: format!(
                    "the answer is `{other}`, which this build does not know. Read as not \
                     determined rather than as agreement, because a word it cannot check is \
                     not a word it may believe"
                ),
            },
        };
        Ok(Record {
            asked: field("asked").unwrap_or_default(),
            at: field("at").unwrap_or_default(),
            software: field("software").unwrap_or_default(),
            reference: field("reference").unwrap_or_default(),
            backend: field("backend").unwrap_or_default(),
            version: field("version").unwrap_or_default(),
            took: field("took_seconds")
                .and_then(|n| n.parse::<f64>().ok())
                .map(Duration::from_secs_f64)
                .unwrap_or_default(),
            answer,
        })
    }
}

/// A subject fit for a file name. Anything else becomes a dash.
///
/// Not refused, unlike an anchor's name: this is a label in a file name and not
/// a directory a write lands in, and a run that could not be recorded because
/// its subject had a slash would be a lost answer in exchange for nothing.
fn sanitised(subject: &str) -> String {
    let cleaned: String = subject
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awaseru_core::verdict::{Difference, Undetermined};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("awaseru-record-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    fn record(answer: Answer) -> Record {
        Record {
            asked: "--anchor settled --region work-ram".into(),
            at: "20261003-153000-001".into(),
            software: "c6858d5c".into(),
            reference: "ref-a".into(),
            backend: "mesence".into(),
            version: "2.2.1".into(),
            took: Duration::from_millis(1234),
            answer,
        }
    }

    /// The one that matters most, so it is the one written first.
    ///
    /// §2.3 says the third value is never collapsed. A record that went to
    /// disk as not determined and came back as agreement would make this whole
    /// tool worse than no tool, because a pass nobody earned is worse than no
    /// answer.
    #[test]
    fn not_determined_goes_to_disk_and_comes_back_not_determined() {
        let dir = scratch("undetermined");
        let kept = record(Answer::NotDetermined {
            because: Undetermined::Vacuous { compared: 0 }.to_string(),
        });
        let at = kept.keep(&dir, "compare", "work-ram").expect("it writes");

        let read = Record::read(&at).expect("it reads");
        assert_eq!(read, kept, "everything survives, not only the answer");
        assert!(!read.answer.is_agreement());
        match &read.answer {
            Answer::NotDetermined { because } => {
                assert!(!because.is_empty(), "and it says why");
            }
            other => panic!("came back as {other:?}, which is the failure this guards"),
        }
    }

    /// §8.6's tolerance meeting §2.3's third value.
    ///
    /// A later version may write an answer this build has never heard of. The
    /// tolerant thing is to read the record; the honest thing is to not call
    /// the unknown word agreement.
    #[test]
    fn an_answer_this_build_does_not_know_reads_as_not_determined() {
        let text = record(Answer::Agrees {
            compared: 8,
            moved: 8,
        })
        .to_text()
        .replace("answer = \"agrees\"", "answer = \"agrees-with-reservations\"");

        let read = Record::parse(&text).expect("a record it can still read");
        assert!(
            !read.answer.is_agreement(),
            "an unknown answer must never be agreement"
        );
        match &read.answer {
            Answer::NotDetermined { because } => {
                assert!(
                    because.contains("agrees-with-reservations"),
                    "and it names the word it did not know, said: {because}"
                );
            }
            other => panic!("got {other:?}"),
        }
        // The rest of the record still read, which is what tolerance means.
        assert_eq!(read.software, "c6858d5c");
        assert_eq!(read.version, "2.2.1");
    }

    /// §2.2: agreement over nothing is vacuous, so `moved` travels with it. A
    /// record claiming agreement and not saying how much moved is refused
    /// rather than read as agreement over an unknown amount.
    #[test]
    fn agreement_without_the_number_that_makes_it_a_measurement_is_refused() {
        let whole = record(Answer::Agrees {
            compared: 16,
            moved: 4,
        })
        .to_text();
        let read = Record::parse(&whole).expect("it reads");
        assert_eq!(
            read.answer,
            Answer::Agrees {
                compared: 16,
                moved: 4
            }
        );

        let without = whole
            .lines()
            .filter(|line| !line.starts_with("moved ="))
            .collect::<Vec<_>>()
            .join("\n");
        let why = Record::parse(&without).expect_err("agreement with no movement");
        assert!(why.contains("§2.2"), "said: {why}");
    }

    #[test]
    fn each_answer_survives_the_round_trip_with_what_it_carries() {
        for answer in [
            Answer::Arrived {
                how: "resumed from a cached blob".into(),
                state: "44d8cc475ad11127".into(),
            },
            Answer::Agrees {
                compared: 256,
                moved: 12,
            },
            Answer::Differs {
                because: Difference::new(3, 0xAA, 0xBB, 1, 256).to_string(),
            },
            Answer::NotDetermined {
                because: "nothing was compared".into(),
            },
        ] {
            let kept = record(answer.clone());
            let read = Record::parse(&kept.to_text()).expect("it reads");
            assert_eq!(read.answer, answer, "{} did not survive", kept.answer.word());
            assert_eq!(read.took, Duration::from_millis(1234));
        }
    }

    /// A verdict becomes an answer without losing which of the three it was.
    #[test]
    fn a_verdict_becomes_the_answer_it_is_and_never_a_different_one() {
        assert!(Answer::from(&Verdict::Agrees {
            compared: 4,
            moved: 2
        })
        .is_agreement());

        let undetermined = Answer::from(&Verdict::NotDetermined(Undetermined::Vacuous {
            compared: 0,
        }));
        assert!(!undetermined.is_agreement());
        assert_eq!(undetermined.word(), "not-determined");

        let differs = Answer::from(&Verdict::Differs(Difference::new(0, 1, 2, 1, 4)));
        assert_eq!(differs.word(), "differs");
        assert!(!differs.is_agreement());
    }

    #[test]
    fn the_name_puts_runs_in_order_and_a_subject_that_is_not_a_name_is_still_kept() {
        let dir = scratch("names");
        let first = record(Answer::Arrived {
            how: "replayed".into(),
            state: "a".into(),
        });
        let one = first.keep(&dir, "arrive", "settled").expect("written");
        assert!(
            one.file_name().unwrap().to_string_lossy().starts_with("001-arrive-settled-"),
            "{}",
            one.display()
        );

        let two = first.keep(&dir, "read", "work-ram/0x10").expect("written");
        let name = two.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("002-read-work-ram-0x10-"), "{name}");
        assert!(
            Record::read(&two).is_ok(),
            "a subject with a slash is still a run that happened"
        );
    }
}
