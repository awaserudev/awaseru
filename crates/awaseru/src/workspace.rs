//! A session: one named place where one piece of work keeps everything it has.
//!
//! The type is `Session`; the module is not, because `session` is already the
//! name of one pass through the host (`session.rs`, from M0) and a parameter or
//! a module that shipped does not get renamed to make room.
//!
//! ## Why a session exists
//!
//! The backend's scratch and the anchor cache used to default to two fixed
//! paths under the system's temporary directory, which meant that **every**
//! invocation on the machine wrote into the same two places regardless of which
//! software, which anchor or which piece of work it belonged to. Nobody chose
//! that; it is a default that was never revisited, and `doc/findings.md`'s
//! twenty-fourth, twenty-sixth and twenty-seventh entries are three symptoms of
//! it. §6.7 already said the cache must not be shared, because a blob is the
//! one artefact where a stale copy is invisible — the default obeyed the letter
//! of that, since nobody had configured anything, and broke its reason.
//!
//! A session is the fix: one directory, named by the person, holding one piece
//! of work against one piece of software. Two sessions are two directories, so
//! a collision between them stops being guarded against and stops being
//! possible.
//!
//! ## What a session does not do
//!
//! It never looks at another session. Nothing here searches nearby
//! directories, infers that two sessions are "probably the same work", or takes
//! a blob from somewhere a person did not name. Replaying because a blob was
//! not found is the correct outcome of asking for work nobody has done here
//! yet, and §4.12 requires the run to say so rather than hide it.

use std::fmt;
use std::path::{Path, PathBuf};

/// The file that says a session is open, and who by.
const LOCK: &str = "lock";
/// What a session is of: the software, the reference, when it began.
const DESCRIPTION: &str = "session.toml";

/// What a session is of, as its description says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub software: String,
    pub reference: String,
    pub backend: String,
    pub version: String,
}

/// One named place.
///
/// Open while the value is alive. Dropping it releases the lock, and a process
/// that dies without dropping leaves the lock behind — see `SessionError::Busy`
/// for why that is reported rather than guessed about.
#[derive(Debug)]
pub struct Session {
    dir: PathBuf,
    name: String,
    /// Whether this value is the one holding the lock, so that a clone or a
    /// failed open never releases somebody else's.
    holds_lock: bool,
}

#[derive(Debug)]
pub enum SessionError {
    /// The path given has no last component to take a name from.
    Unnamed { dir: PathBuf },
    /// A name the filesystem cannot carry, or that would not be one component.
    Name { given: String, why: &'static str },
    /// Somebody else has this session open — or had it, and did not get to say
    /// otherwise.
    Busy { lock: PathBuf, holder: String },
    /// This session is of other software than the one now configured.
    OtherSoftware {
        name: String,
        held: String,
        offered: String,
    },
    /// The directory would not do what was asked of it.
    Io { at: PathBuf, why: std::io::Error },
    /// The description is there and will not parse.
    Unreadable { at: PathBuf, why: String },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Unnamed { dir } => write!(
                f,
                "`{}` has no last part to name a session after. A session is named, because a \
                 name is what makes it findable tomorrow and what you hand over",
                dir.display()
            ),
            SessionError::Name { given, why } => write!(
                f,
                "`{given}` cannot name a session, because {why}"
            ),
            SessionError::Busy { lock, holder } => write!(
                f,
                "this session is open: {holder}. One session is one piece of work and one \
                 reference, so two at once would be two processes writing one anchor. If that \
                 process is gone — a machine that stopped never got to say so — remove `{}` and \
                 open it again. Whether it is still running is not something guessed at here",
                lock.display()
            ),
            SessionError::OtherSoftware {
                name,
                held,
                offered,
            } => write!(
                f,
                "the session `{name}` is of the software {held} and was offered {offered}. A \
                 session holds one piece of software, which is what lets its anchors be named \
                 after what they are; start another session rather than reusing this one"
            ),
            SessionError::Io { at, why } => {
                write!(f, "`{}` could not be used: {why}", at.display())
            }
            SessionError::Unreadable { at, why } => write!(
                f,
                "`{}` is there and is not something this tool wrote: {why}",
                at.display()
            ),
        }
    }
}

impl std::error::Error for SessionError {}

impl Session {
    /// Opens the session at `dir`, creating it if it is not there.
    ///
    /// The name is the path's last component, so that one option carries both
    /// the name and the place and there is no hidden root anywhere for sessions
    /// to collect in. A person who can see the directory can compress it.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Session, SessionError> {
        let dir = dir.into();
        let name = dir
            .file_name()
            .and_then(|part| part.to_str())
            .ok_or_else(|| SessionError::Unnamed { dir: dir.clone() })?
            .to_string();
        if let Some(why) = unusable_as_a_name(&name) {
            return Err(SessionError::Name { given: name, why });
        }

        for part in [Path::new(""), Path::new("home"), Path::new("anchors"), Path::new("runs"), Path::new("logs")] {
            let at = dir.join(part);
            std::fs::create_dir_all(&at).map_err(|why| SessionError::Io { at, why })?;
        }

        let lock = dir.join(LOCK);
        // `create_new` is the whole of the exclusion: the filesystem decides,
        // and two processes arriving together cannot both be told yes.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                use std::io::Write;
                let holder = format!("process {}", std::process::id());
                let _ = writeln!(file, "{holder}");
            }
            Err(why) if why.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = std::fs::read_to_string(&lock)
                    .map(|text| text.trim().to_string())
                    .unwrap_or_else(|_| "by something that left no name".to_string());
                return Err(SessionError::Busy { lock, holder });
            }
            Err(why) => return Err(SessionError::Io { at: lock, why }),
        }

        Ok(Session {
            dir,
            name,
            holds_lock: true,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// Where the backend may keep its own files. One per session, so the
    /// twenty-seventh finding cannot happen between two of them.
    pub fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    /// Where blobs live (§4.11). Inside the session, because a cache that two
    /// pieces of work shared is the thing §6.7 refuses.
    pub fn anchors(&self) -> PathBuf {
        self.dir.join("anchors")
    }

    /// Where what was asked and what came back is kept.
    pub fn runs(&self) -> PathBuf {
        self.dir.join("runs")
    }

    pub fn logs(&self) -> PathBuf {
        self.dir.join("logs")
    }

    /// What this session was recorded as being of, if it has been.
    ///
    /// Read back from the file rather than kept in memory, because a session
    /// opened today was described on whatever day it was first opened and the
    /// file is where that lives.
    pub fn described(&self) -> Option<Described> {
        let text = std::fs::read_to_string(self.dir.join(DESCRIPTION)).ok()?;
        Some(Described {
            software: field(&text, "software")?.to_string(),
            reference: field(&text, "reference").unwrap_or_default().to_string(),
            backend: field(&text, "backend").unwrap_or_default().to_string(),
            version: field(&text, "version").unwrap_or_default().to_string(),
        })
    }

    /// Records what this session is of, or refuses if it is already of
    /// something else.
    ///
    /// Called once the reference has been opened, because that is the first
    /// moment the software's identity and the backend's version are known
    /// rather than assumed. A session that already names other software is
    /// refused: §6.6 says pointing the tool at another revision makes every
    /// comparison meaningless, and a session's anchors are named after
    /// positions in **one** piece of software.
    pub fn describe(
        &self,
        software: &str,
        reference: &str,
        backend: &str,
        version: &str,
    ) -> Result<(), SessionError> {
        let at = self.dir.join(DESCRIPTION);
        if let Ok(text) = std::fs::read_to_string(&at) {
            let held = field(&text, "software").ok_or_else(|| SessionError::Unreadable {
                at: at.clone(),
                why: "it says nothing about which software it is of".to_string(),
            })?;
            if held != software {
                return Err(SessionError::OtherSoftware {
                    name: self.name.clone(),
                    held: held.to_string(),
                    offered: software.to_string(),
                });
            }
            return Ok(());
        }

        let text = format!(
            "# What this session is of. Written when it was first opened.\n\
             #\n\
             # The software is what fixes everything else: the anchors in here are\n\
             # named after positions in it, and §6.6 refuses another revision.\n\
             software = \"{software}\"\n\
             reference = \"{reference}\"\n\
             backend = \"{backend}\"\n\
             version = \"{version}\"\n"
        );
        std::fs::write(&at, text).map_err(|why| SessionError::Io { at, why })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.holds_lock {
            let _ = std::fs::remove_file(self.dir.join(LOCK));
        }
    }
}

/// The moment, as `YYYYMMDD-HHMMSS-mmm`.
///
/// Lives here because two things need it — a log's name, so that one run does
/// not erase the one before it, and a run's record, so that what was asked
/// carries when it was asked. A second copy of the arithmetic below is the one
/// thing worse than the arithmetic.
///
/// Hand-written rather than taken from a crate: a date is four lines of
/// division and §17.2 keeps dependencies to what `doc/dependencies.md` decided.
pub fn stamp() -> String {
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

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{millis:03}")
}

/// Why `name` cannot name a session, or `None` if it can.
///
/// A free function returning the reason, so both answers are testable and the
/// refusal can say which rule was broken. The same rule as an anchor's name,
/// and for the same reason: it becomes a directory.
pub fn unusable_as_a_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("it is empty");
    }
    if name == "." || name == ".." {
        return Some("it names a directory instead of being one");
    }
    if name.contains('/') || name.contains('\\') {
        return Some("it carries a path separator");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some("it has a character outside letters, digits, `-` and `_`");
    }
    None
}

/// One `key = "value"` out of the description.
///
/// Hand-read rather than parsed into a type, because the description has four
/// fields and a struct for them would be four more places to keep in step.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim().trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("awaseru-session-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    #[test]
    fn opening_makes_the_places_and_takes_its_name_from_the_path() {
        let at = scratch("open").join("ff5-battle");
        let session = Session::open(&at).expect("it opens");

        assert_eq!(session.name(), "ff5-battle");
        for place in [session.home(), session.anchors(), session.runs(), session.logs()] {
            assert!(place.is_dir(), "{} should be there", place.display());
        }
        assert!(at.join(LOCK).is_file(), "and it says it is open");
    }

    /// The refusal, made to happen rather than asserted around.
    #[test]
    fn a_session_already_open_is_refused_and_says_how_to_recover() {
        let at = scratch("busy").join("work");
        let held = Session::open(&at).expect("it opens");

        let err = Session::open(&at).expect_err("twice is refused");
        match &err {
            SessionError::Busy { lock, holder } => {
                assert_eq!(lock, &at.join(LOCK));
                assert!(
                    holder.contains(&std::process::id().to_string()),
                    "it names who has it, said: {holder}"
                );
            }
            other => panic!("got {other}"),
        }
        // The message has to carry the way out, because nothing here decides
        // whether the other process is alive.
        let said = err.to_string();
        assert!(said.contains("remove"), "said: {said}");
        assert!(said.contains("not something guessed at here"), "said: {said}");

        drop(held);
        Session::open(&at).expect("released, so it opens again");
    }

    #[test]
    fn a_failed_open_does_not_release_the_lock_it_did_not_take() {
        let at = scratch("not-mine").join("work");
        let held = Session::open(&at).expect("it opens");
        {
            let refused = Session::open(&at);
            assert!(refused.is_err());
        }
        assert!(
            at.join(LOCK).is_file(),
            "the refused open must not have taken the holder's lock away"
        );
        drop(held);
    }

    #[test]
    fn a_name_that_cannot_be_a_directory_is_refused_and_a_plain_one_is_not() {
        assert_eq!(unusable_as_a_name("ff5-battle"), None);
        assert_eq!(unusable_as_a_name("work_2"), None);
        assert_eq!(unusable_as_a_name(""), Some("it is empty"));
        assert_eq!(
            unusable_as_a_name(".."),
            Some("it names a directory instead of being one")
        );
        assert_eq!(
            unusable_as_a_name("a b"),
            Some("it has a character outside letters, digits, `-` and `_`")
        );

        let err = Session::open(scratch("bad-name").join("..")).expect_err("refused");
        assert!(
            matches!(
                err,
                SessionError::Name { .. } | SessionError::Unnamed { .. }
            ),
            "got {err}"
        );
    }

    #[test]
    fn what_a_session_is_of_is_written_once_and_other_software_is_refused() {
        let at = scratch("describe").join("work");
        let session = Session::open(&at).expect("it opens");

        session
            .describe("c6858d5c", "ref-a", "mesence", "2.2.1")
            .expect("it records");
        let text = std::fs::read_to_string(at.join(DESCRIPTION)).expect("it is there");
        assert!(text.contains("software = \"c6858d5c\""), "said: {text}");

        session
            .describe("c6858d5c", "ref-a", "mesence", "2.2.1")
            .expect("the same software is the same session");

        let err = session
            .describe("00000000", "ref-a", "mesence", "2.2.1")
            .expect_err("other software is refused");
        match &err {
            SessionError::OtherSoftware { held, offered, .. } => {
                assert_eq!(held, "c6858d5c");
                assert_eq!(offered, "00000000");
            }
            other => panic!("got {other}"),
        }
    }

    /// U1's done-condition, and the reason the whole frente exists: two pieces
    /// of work against the same software share **nothing**, so neither can find
    /// what the other did.
    #[test]
    fn two_sessions_of_one_software_touch_none_of_each_others_files() {
        let root = scratch("separate");
        let one = Session::open(root.join("no-job")).expect("it opens");
        let two = Session::open(root.join("with-job")).expect("it opens too");

        one.describe("c6858d5c", "ref-a", "mesence", "2.2.1")
            .expect("recorded");
        two.describe("c6858d5c", "ref-a", "mesence", "2.2.1")
            .expect("recorded");

        assert_ne!(one.anchors(), two.anchors());
        assert_ne!(one.home(), two.home());
        assert_ne!(one.runs(), two.runs());
        assert_ne!(one.logs(), two.logs());

        // Something one of them keeps is not something the other can see. This
        // is the point: the second session REPLAYS, because nothing told it
        // where the first one's work is, and nothing here goes looking.
        std::fs::write(one.anchors().join("in-control"), b"a blob").expect("written");
        assert!(
            !two.anchors().join("in-control").exists(),
            "the other session must not be able to find it"
        );
    }
}
