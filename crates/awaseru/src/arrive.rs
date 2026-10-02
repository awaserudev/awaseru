//! Arriving at an anchor, and showing that arriving the cheap way is the same
//! as arriving the expensive way — §4.7, §4.8, §4.9, §4.12.
//!
//! # The two ways, and why they must not differ
//!
//! A reference reaches an anchor either by **replaying** its definition from
//! the origin or by **resuming** a cached blob. §4.7 says which of those
//! happened must never change the result, and §4.8 says the whole reason the
//! question is dangerous: if the cached state is not the state a replay would
//! have produced, every comparison below it measures the wrong machine **and
//! passes**.
//!
//! So an anchor carries a demonstration, and until it does, every verdict from
//! a run that began there is *not determined* (§2.3). That is a value this
//! module hands back, not a warning somebody might read.
//!
//! # What a "state" means here
//!
//! A `Witness`: the position, a digest of the regions the anchor declares, and
//! a digest of the processor record. The last of those matters more than it
//! looks — the record begins with a cycle count, so two machines that did the
//! same work in different numbers of cycles do not pass for each other.

use std::time::{Duration, Instant};

use awaseru_core::anchor::{Anchor, AnchorError, Anchors, CheapCheck, CheckFailed, Coverage, Key};
use awaseru_core::run::Position;
use awaseru_core::snapshot::Provenance;
use awaseru_core::{Beginning, Platform, ReadError, RunError, StateError, Undetermined};
use sha2::{Digest, Sha256};

use crate::cache::{Cache, CacheError, Stored};
use crate::config::AnchorPolicy;

/// Everything readable about where a machine is, cheaply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witness {
    pub position: Position,
    pub coverage: Coverage,
    /// A digest of the processor record. It begins with a cycle count, so this
    /// is what stops two machines that took different numbers of cycles from
    /// passing for each other.
    pub processor: String,
}

impl Witness {
    /// `position` is the caller's, because the trait has no "where are you"
    /// and should not — a position is what a run arrived at, and the run that
    /// arrived knows it (§4.3).
    pub fn of(
        platform: &dyn Platform,
        covers: &[String],
        position: Position,
    ) -> Result<Self, ReadError> {
        let coverage = Coverage::of(platform, covers)?;
        let processor = platform.read_processor()?;
        let out = Sha256::digest(processor.bytes());
        let hex = out.iter().fold(String::with_capacity(64), |mut s, byte| {
            use std::fmt::Write;
            let _ = write!(s, "{byte:02x}");
            s
        });
        Ok(Witness {
            position,
            coverage,
            processor: hex,
        })
    }

    /// What differs, in words, for a report that has to say why a
    /// demonstration failed.
    pub fn differences(&self, other: &Witness) -> Vec<String> {
        let mut out = Vec::new();
        if self.position != other.position {
            out.push(format!(
                "the position: {} against {}",
                self.position, other.position
            ));
        }
        if self.processor != other.processor {
            out.push("the processor record".to_string());
        }
        for region in self.coverage.differing(&other.coverage) {
            out.push(format!("the region `{region}`"));
        }
        out
    }
}

/// How a reference got somewhere — §4.12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The definition was replayed from the origin.
    Replayed { anchors_run: usize },
    /// A cached blob was resumed.
    Resumed,
}

impl std::fmt::Display for How {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            How::Replayed { anchors_run } => write!(
                f,
                "replayed from the origin through {anchors_run} definition(s)"
            ),
            How::Resumed => write!(f, "resumed from a cached blob"),
        }
    }
}

/// What happened on the way to an anchor — §4.12.
#[derive(Debug, Clone)]
pub struct Arrived {
    pub anchor: String,
    pub how: How,
    pub took: Duration,
    /// How the reference came up — §4.12 wants this next to the result, not in
    /// a footnote somebody has to find.
    pub beginning: Beginning,
    /// Present when the anchor has not been shown equivalent (§4.8). **Every
    /// verdict from this run must carry it**; it is returned rather than logged
    /// so that ignoring it is a value dropped where a reviewer can see.
    pub caveat: Option<Undetermined>,
    /// Set when this arrival re-ran the demonstration because §4.9's
    /// `reverify_after` came due.
    pub reverified: bool,
}

impl std::fmt::Display for Arrived {
    /// §4.12's line: how it arrived, how long that took, and what the run is
    /// worth.
    ///
    /// The caveat and the beginning come **last and in words**, because the
    /// thing a reader skips is the thing at the end of a line of numbers — so
    /// what is at the end is what they need most when it is bad.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "anchor `{}`: {} in {:.3}s",
            self.anchor,
            self.how,
            self.took.as_secs_f64()
        )?;
        if self.reverified {
            write!(f, "; re-demonstrated first, which §4.9 had come due")?;
        }
        write!(f, ". It {}", self.beginning)?;
        if let Some(caveat) = &self.caveat {
            write!(f, ". NOT DETERMINED: {caveat}")?;
        }
        Ok(())
    }
}

impl Arrived {
    /// Whether a comparison from this run is worth anything (§2.3, §2.5).
    ///
    /// Two separate reasons it may not be, and both have to be false: the
    /// anchor was never demonstrated (§4.8), or the reference did not come up
    /// somewhere that repeats (§2.5).
    pub fn is_evidence(&self) -> bool {
        self.caveat.is_none() && self.beginning.repeats()
    }
}

/// Why an arrival or a demonstration failed.
#[derive(Debug)]
pub enum ArriveError {
    Anchors(AnchorError),
    Read(ReadError),
    Run(RunError),
    State(StateError),
    Cache(CacheError),
    /// A bound an anchor asked for is one the backend will not honour, so the
    /// anchor cannot be reached at all.
    Refused { anchor: String, why: String },
    /// A replay of one definition did not agree with another replay of the same
    /// definition. The anchor's derivation is not deterministic, so it is not
    /// an anchor (§2.5, §4.8).
    ReplaysDisagree { anchor: String, differences: Vec<String> },
    /// Resuming the blob did not give what replaying gives. **This is the
    /// failure §4.8 exists to catch**, and it is the one that would otherwise
    /// pass.
    ResumeDisagrees { anchor: String, differences: Vec<String> },
    /// Both arrived at the same place and running onward from them parted. A
    /// blob that restores what the anchor declares and not the rest looks
    /// exactly like this.
    OnwardDisagrees { anchor: String, differences: Vec<String> },
}

impl std::fmt::Display for ArriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArriveError::Anchors(e) => write!(f, "{e}"),
            ArriveError::Read(e) => write!(f, "{e}"),
            ArriveError::Run(e) => write!(f, "{e}"),
            ArriveError::State(e) => write!(f, "{e}"),
            ArriveError::Cache(e) => write!(f, "{e}"),
            ArriveError::Refused { anchor, why } => write!(
                f,
                "the anchor `{anchor}` cannot be reached: {why}"
            ),
            ArriveError::ReplaysDisagree {
                anchor,
                differences,
            } => write!(
                f,
                "replaying `{anchor}` twice from the origin gave two different machines, which \
                 differ in {}. An anchor whose derivation is not deterministic is not an anchor \
                 (§2.5)",
                differences.join(", ")
            ),
            ArriveError::ResumeDisagrees {
                anchor,
                differences,
            } => write!(
                f,
                "resuming `{anchor}` from its blob is not the same machine as replaying its \
                 definition: {} differ. **This is the failure §4.8 exists to catch** — without \
                 the check, every comparison below this anchor would measure the wrong machine \
                 and pass",
                differences.join(", ")
            ),
            ArriveError::OnwardDisagrees {
                anchor,
                differences,
            } => write!(
                f,
                "replaying and resuming `{anchor}` arrive at the same place, and running onward \
                 from them parts: {} differ. The blob restores what the anchor declares and \
                 something it does not",
                differences.join(", ")
            ),
        }
    }
}

impl std::error::Error for ArriveError {}

impl From<AnchorError> for ArriveError {
    fn from(e: AnchorError) -> Self {
        ArriveError::Anchors(e)
    }
}
impl From<ReadError> for ArriveError {
    fn from(e: ReadError) -> Self {
        ArriveError::Read(e)
    }
}
impl From<RunError> for ArriveError {
    fn from(e: RunError) -> Self {
        ArriveError::Run(e)
    }
}
impl From<StateError> for ArriveError {
    fn from(e: StateError) -> Self {
        ArriveError::State(e)
    }
}
impl From<CacheError> for ArriveError {
    fn from(e: CacheError) -> Self {
        ArriveError::Cache(e)
    }
}

/// What a demonstration established — §4.8.
#[derive(Debug, Clone)]
pub struct Demonstration {
    pub anchor: String,
    /// How many replays agreed with each other.
    pub replays: u32,
    pub took: Duration,
    /// Whether the fresh-process half of §4.8's step three was done here.
    ///
    /// Always `false`: this runs in one process, and a library that spawned
    /// processes to prove a point would be a surprising library. The
    /// across-processes half is M2's done-condition test, which spawns them.
    pub across_processes: bool,
}

/// Drives a reference to anchors, and demonstrates them.
pub struct Arriver<'a> {
    platform: &'a mut dyn Platform,
    anchors: &'a Anchors,
    cache: &'a Cache,
    provenance: Provenance,
    policy: AnchorPolicy,
    /// The last position the platform reported, because the trait has no way to
    /// ask (§4.3) and every run returns one.
    at: Position,
}

impl<'a> Arriver<'a> {
    pub fn new(
        platform: &'a mut dyn Platform,
        anchors: &'a Anchors,
        cache: &'a Cache,
        provenance: Provenance,
        policy: AnchorPolicy,
    ) -> Self {
        Arriver {
            platform,
            anchors,
            cache,
            provenance,
            policy,
            at: Position::Unclassified { pc: 0 },
        }
    }

    pub fn at(&self) -> &Position {
        &self.at
    }

    /// Advances the reference, keeping the position this holds up to date.
    ///
    /// Here because an `Arriver` borrows the platform for as long as it lives,
    /// and because the position it keeps is the one every witness and snapshot
    /// is labelled with — a run that went around it would leave that stale.
    pub fn run(&mut self, bound: awaseru_core::Bound) -> Result<awaseru_core::Stop, RunError> {
        let stop = self.platform.run(bound)?;
        self.at = stop.position.clone();
        Ok(stop)
    }

    /// Writes a span, through the reference this is driving.
    pub fn write_span(
        &mut self,
        region: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), awaseru_core::WriteError> {
        self.platform.write_span(region, offset, bytes)
    }

    /// Captures the named spans at the position this holds.
    pub fn capture(
        &self,
        provenance: awaseru_core::snapshot::Provenance,
        spans: &[(&str, usize, usize)],
    ) -> Result<awaseru_core::Snapshot, awaseru_core::CaptureError> {
        awaseru_core::capture_spans(self.platform, provenance, self.at.clone(), spans)
    }

    /// A region, through the reference this is driving.
    ///
    /// Here because an `Arriver` borrows the platform for as long as it lives,
    /// so a caller that wants to look at what it arrived at has to ask it.
    pub fn read(&self, region: &str) -> Result<Vec<u8>, ReadError> {
        self.platform.read(region)
    }

    /// A witness of where the machine is now, over the given regions.
    fn witness(&self, covers: &[String]) -> Result<Witness, ArriveError> {
        Ok(Witness::of(self.platform, covers, self.at.clone())?)
    }

    /// Every writable region the backend exposes.
    ///
    /// **This, and not the anchor's declared regions, is what a demonstration
    /// compares over** — and the difference is the whole reason the
    /// demonstration exists separately from the cheap check.
    ///
    /// The cheap check (§4.8) runs on every load and is deliberately narrow:
    /// what the anchor declares. The demonstration runs once per key and can
    /// afford to be thorough, so it should be, or it establishes less than
    /// anybody reading "demonstrated" will assume.
    ///
    /// Found the hard way. With the demonstration comparing only the declared
    /// regions, a resume that quietly scrambled a region the anchor did not
    /// declare passed every check this tool had — including the
    /// across-processes one, because both paths finish by resuming and the
    /// damage was identical on each. Comparing everything writable is what
    /// catches it.
    fn everything_writable(&self) -> Vec<String> {
        self.platform
            .regions()
            .iter()
            .filter(|r| r.access.writable())
            .map(|r| r.name.clone())
            .collect()
    }

    /// Replays a chain from the origin, leaving the machine at its end.
    fn replay(&mut self, chain: &[&Anchor]) -> Result<usize, ArriveError> {
        self.platform.return_to_origin()?;
        for anchor in chain {
            let stop = self.platform.run(anchor.definition.bound.clone())?;
            if !stop.arrived() {
                return Err(ArriveError::Refused {
                    anchor: anchor.name.clone(),
                    why: stop.to_string(),
                });
            }
            self.at = stop.position;
        }
        Ok(chain.len())
    }

    /// Brings the reference to `name`, the cheap way when that is allowed.
    ///
    /// # Where it leaves the machine, and what `how` therefore means
    ///
    /// Always at **the blob's position** — the one the cheap check records and
    /// a resume reproduces — whichever way the blob was obtained. So `how`
    /// describes how the *blob* was got, which is where the time went (§4.12),
    /// and not a difference in where the machine ends up. §4.7 requires those
    /// to be the same place, and the demonstration is what establishes that
    /// they are.
    ///
    /// Resumes a cached blob when there is one whose key matches and whose
    /// cheap check passes; replays otherwise. When the anchor has never been
    /// demonstrated and §4.9's `verify_from_origin` is not zero, it is
    /// demonstrated first — §4.8 says an anchor carries a demonstration, and
    /// the tool is what carries it rather than the user remembering to ask.
    pub fn arrive(&mut self, name: &str) -> Result<Arrived, ArriveError> {
        let began = Instant::now();
        let key = self.anchors.key(name, &self.provenance)?;
        let anchor = self
            .anchors
            .get(name)
            .ok_or_else(|| AnchorError::Unknown {
                name: name.to_string(),
                declared: self.anchors.names().map(str::to_string).collect(),
            })?
            .clone();

        // §4.9: a demonstration that has come due is run before the arrival it
        // is about, of the tool's own accord.
        let mut reverified = false;
        if let Some(stored) = self.cache.get(&key) {
            let due = self.policy.reverify_after > 0 && stored.uses >= self.policy.reverify_after;
            let never = stored.demonstrated_with == 0 && self.policy.verify_from_origin > 0;
            if due || never {
                self.demonstrate(name)?;
                reverified = due;
            }
        }

        if let Some(stored) = self.cache.get(&key) {
            match self.resume(&stored, &anchor) {
                Ok(()) => {
                    self.cache.note_use(&key);
                    return Ok(Arrived {
                        anchor: name.to_string(),
                        how: How::Resumed,
                        took: began.elapsed(),
                        beginning: self.platform.beginning(),
                        caveat: caveat_for(name, stored.demonstrated_with),
                        reverified,
                    });
                }
                Err(_) => {
                    // The blob is stale or the load did nothing. §4.11 says the
                    // cache is never an input, so this is a miss: throw it away
                    // and replay.
                    self.cache.forget(&key);
                }
            }
        }

        let chain = self.anchors.chain(name)?.into_iter().cloned().collect::<Vec<_>>();
        let chain_refs: Vec<&Anchor> = chain.iter().collect();
        let anchors_run = self.replay(&chain_refs)?;
        self.store(&key, &anchor)?;

        // Demonstrating after a replay that has just produced the blob, when
        // the policy asks for one and nothing has done it.
        let demonstrated = if self.policy.verify_from_origin > 0 {
            self.demonstrate(name)?;
            self.policy.verify_from_origin
        } else {
            0
        };

        Ok(Arrived {
            anchor: name.to_string(),
            how: How::Replayed { anchors_run },
            took: began.elapsed(),
            beginning: self.platform.beginning(),
            caveat: caveat_for(name, demonstrated),
            reverified,
        })
    }

    /// Loads a blob and runs §4.8's cheap check against it.
    fn resume(&mut self, stored: &Stored, anchor: &Anchor) -> Result<(), ArriveError> {
        self.platform.load_state(&stored.blob)?;
        self.at = stored.blob.position().clone();
        let coverage = Coverage::of(self.platform, &anchor.covers)?;
        stored
            .check
            .verify(&self.at, &coverage)
            .map_err(|e: CheckFailed| ArriveError::State(StateError::Backend { why: e.to_string() }))
    }

    /// Takes a blob where the machine stands and caches it under `key`.
    fn store(&mut self, key: &Key, anchor: &Anchor) -> Result<(), ArriveError> {
        let blob = self.platform.save_state()?;
        // After the save, never before — saving advances the machine
        // (`doc/backend.md`).
        self.at = blob.position().clone();
        let coverage = Coverage::of(self.platform, &anchor.covers)?;
        let stored = Stored {
            check: CheapCheck {
                position: self.at.clone(),
                coverage,
            },
            blob,
            uses: 0,
            demonstrated_with: 0,
        };
        self.cache.put(key, &stored)?;
        Ok(())
    }

    /// §4.8's demonstration, steps one to four.
    ///
    /// Step five — deleting the cache and finding that everything still passes
    /// — is a property of the whole arrangement rather than something this can
    /// do to itself, and it is asserted by M2's done-condition test.
    pub fn demonstrate(&mut self, name: &str) -> Result<Demonstration, ArriveError> {
        let began = Instant::now();
        let replays = self.policy.verify_from_origin.max(1);
        let anchor = self
            .anchors
            .get(name)
            .ok_or_else(|| AnchorError::Unknown {
                name: name.to_string(),
                declared: self.anchors.names().map(str::to_string).collect(),
            })?
            .clone();
        let chain = self.anchors.chain(name)?.into_iter().cloned().collect::<Vec<_>>();
        let chain_refs: Vec<&Anchor> = chain.iter().collect();
        let key = self.anchors.key(name, &self.provenance)?;

        // ---- step 1: the definition is deterministic -------------------
        // Everything writable, not the anchor's declared regions — see
        // `everything_writable` for what that cost before it was fixed.
        let watched = self.everything_writable();

        let mut replayed: Option<Witness> = None;
        for _ in 0..replays {
            self.replay(&chain_refs)?;
            let witness = self.witness(&watched)?;
            if let Some(first) = &replayed {
                let differences = first.differences(&witness);
                if !differences.is_empty() {
                    return Err(ArriveError::ReplaysDisagree {
                        anchor: name.to_string(),
                        differences,
                    });
                }
            } else {
                replayed = Some(witness);
            }
        }
        debug_assert!(
            replayed.is_some(),
            "`replays` is at least one, so a witness was taken"
        );

        // ---- step 2: take the blob and cache it ------------------------
        self.store(&key, &anchor)?;
        let stored = self
            .cache
            .get(&key)
            .expect("just stored, and the cache round-trips");
        // The blob's position is after the save, so a witness of the replay has
        // to be taken there too — otherwise step 3 compares the position before
        // the save against the position after it and always disagrees.
        let replayed_at_blob = self.witness(&watched)?;

        // ---- step 3: resuming gives the same machine ------------------
        self.resume(&stored, &anchor)?;
        let resumed = self.witness(&watched)?;
        let differences = replayed_at_blob.differences(&resumed);
        if !differences.is_empty() {
            return Err(ArriveError::ResumeDisagrees {
                anchor: name.to_string(),
                differences,
            });
        }

        // ---- step 4: and running onward from each agrees --------------
        // The step that matters. Agreeing *at* the anchor is not the same as
        // agreeing after running on from it: a blob can restore the memories
        // an anchor declares and leave something it does not.
        let onward = anchor.definition.bound.clone();
        let stop = self.platform.run(onward.clone())?;
        self.at = stop.position;
        let from_resume = self.witness(&watched)?;

        self.replay(&chain_refs)?;
        let stop = self.platform.run(onward)?;
        self.at = stop.position;
        let from_replay = self.witness(&watched)?;

        let differences = from_resume.differences(&from_replay);
        if !differences.is_empty() {
            return Err(ArriveError::OnwardDisagrees {
                anchor: name.to_string(),
                differences,
            });
        }

        // Recorded against the blob, so a later arrival knows it was done and
        // how many replays stood behind it.
        let mut stored = stored;
        stored.demonstrated_with = replays;
        stored.uses = 0;
        self.cache.put(&key, &stored)?;

        // Leave the machine at the anchor rather than past it.
        self.resume(&stored, &anchor)?;

        Ok(Demonstration {
            anchor: name.to_string(),
            replays,
            took: began.elapsed(),
            across_processes: false,
        })
    }

}

/// §4.8's "until it has one, comparisons made from it are *not determined*".
fn caveat_for(anchor: &str, demonstrated_with: u32) -> Option<Undetermined> {
    (demonstrated_with == 0).then(|| Undetermined::AnchorNotDemonstrated {
        anchor: anchor.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrived(how: How, caveat: Option<Undetermined>, repeats: bool) -> Arrived {
        Arrived {
            anchor: "later".into(),
            how,
            took: Duration::from_millis(1234),
            beginning: Beginning {
                reproducible: repeats,
                settled: if repeats {
                    vec!["work-ram".into()]
                } else {
                    Vec::new()
                },
            },
            caveat,
            reverified: false,
        }
    }

    /// §4.12's line says how it arrived and how long. The number is there so
    /// somebody can see where the time went without a stopwatch, which is the
    /// whole reason §4.12 exists.
    #[test]
    fn the_line_says_how_it_arrived_and_how_long_that_took() {
        let replayed = arrived(How::Replayed { anchors_run: 2 }, None, true).to_string();
        assert!(replayed.contains("replayed"), "{replayed}");
        assert!(replayed.contains("2 definition"), "{replayed}");
        assert!(replayed.contains("1.234s"), "{replayed}");

        let resumed = arrived(How::Resumed, None, true).to_string();
        assert!(resumed.contains("resumed"), "{resumed}");
        assert_ne!(replayed, resumed, "the two ways must not read alike");
    }

    /// **Two separate reasons a run may be worth nothing, and both have to be
    /// absent.** An anchor nobody demonstrated, and a reference that did not
    /// begin somewhere it can return to. A report that checked one and not the
    /// other would pass a run that is not evidence.
    #[test]
    fn a_run_is_evidence_only_when_neither_reason_against_it_holds() {
        let caveat = || {
            Some(Undetermined::AnchorNotDemonstrated {
                anchor: "later".into(),
            })
        };

        assert!(arrived(How::Resumed, None, true).is_evidence());
        assert!(
            !arrived(How::Resumed, caveat(), true).is_evidence(),
            "an undemonstrated anchor is not evidence however reproducibly it began"
        );
        assert!(
            !arrived(How::Resumed, None, false).is_evidence(),
            "a reference that does not repeat is not evidence however demonstrated the anchor"
        );
        assert!(!arrived(How::Resumed, caveat(), false).is_evidence());
    }

    /// When something is wrong, the line ends with it. A reader's eye stops at
    /// the end of a line of numbers, so that is where the thing they need most
    /// has to be.
    #[test]
    fn the_caveat_is_the_last_thing_on_the_line() {
        let said = arrived(How::Resumed, Some(Undetermined::AnchorNotDemonstrated {
            anchor: "later".into(),
        }), true)
        .to_string();
        assert!(said.contains("NOT DETERMINED"), "{said}");
        assert!(
            said.rfind("NOT DETERMINED") > said.rfind("resumed"),
            "the caveat must come after the result, not before it: {said}"
        );

        let clean = arrived(How::Resumed, None, true).to_string();
        assert!(
            !clean.contains("NOT DETERMINED"),
            "and must not appear when there is nothing wrong: {clean}"
        );
    }

    /// A re-demonstration that §4.9 brought due is said, because it is where
    /// the time went on that particular run.
    #[test]
    fn a_reverification_says_it_happened() {
        let mut a = arrived(How::Resumed, None, true);
        a.reverified = true;
        let said = a.to_string();
        assert!(said.contains("re-demonstrated"), "{said}");
        assert!(said.contains("§4.9"), "and says which rule brought it due: {said}");
    }

    /// Two witnesses that differ say what differs, by name, because "they
    /// differ" sends somebody reading and "the region `work-ram`" does not.
    #[test]
    fn two_witnesses_name_what_differs_between_them() {
        let witness = |position, processor: &str, coverage: &[(&str, &str)]| Witness {
            position,
            coverage: Coverage::from_digests(
                coverage
                    .iter()
                    .map(|(n, d)| ((*n).to_string(), (*d).to_string()))
                    .collect(),
            ),
            processor: processor.to_string(),
        };

        let a = witness(
            Position::FrameBoundary { frame: 1 },
            "aa",
            &[("work-ram", "11"), ("palette-ram", "22")],
        );
        assert!(a.differences(&a).is_empty(), "a witness agrees with itself");

        let b = witness(
            Position::FrameBoundary { frame: 2 },
            "bb",
            &[("work-ram", "11"), ("palette-ram", "33")],
        );
        let differences = a.differences(&b);
        assert_eq!(differences.len(), 3, "got {differences:?}");
        assert!(differences.iter().any(|d| d.contains("position")));
        assert!(differences.iter().any(|d| d.contains("processor record")));
        assert!(differences.iter().any(|d| d.contains("palette-ram")));
        assert!(
            !differences.iter().any(|d| d.contains("work-ram")),
            "and does not name what agrees: {differences:?}"
        );
    }
}
