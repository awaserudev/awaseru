//! awaseru's platform-independent half.
//!
//! What lives here knows nothing about any console. It is the vocabulary the
//! rest of the tool speaks: regions a backend names and the host asks for
//! (§3.1), positions and bounds and why a run stopped (§3.4, §4), the
//! three-valued verdict a comparison produces (§2.3), and the trait a backend
//! implements (§7).
//!
//! The one rule that shapes all of it: **no platform name crosses this line**
//! (§2.7). Nothing here says "video memory" or names a manufacturer, because a
//! model with fields for one platform's memories cannot gain a second platform
//! without changing every comparison written against it.
//!
//! See `spec.md`, which is normative.

pub mod platform;
pub mod region;
pub mod run;
pub mod verdict;

pub use platform::{BackendVersion, Platform, ReadError, RunError, check_read};
pub use region::{Access, Region, Regions, SpanError};
pub use run::{Bound, Position, Reason, Stop};
pub use verdict::{Difference, Undetermined, Verdict, fold};
