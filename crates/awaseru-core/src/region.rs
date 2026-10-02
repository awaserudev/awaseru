//! Regions — §3.1.
//!
//! A region is a named, addressable span of bytes that a backend exposes. The
//! host obtains the list by asking, and must not assume which names exist: a
//! model with fields for one platform's memories cannot gain a second platform
//! without changing every comparison written against it.

/// What may be done to a region.
///
/// Write-only is not a theoretical case. Hardware registers that latch a value
/// and read back as something else — or as nothing — are ordinary, and a model
/// that cannot say "you may write here but not read it" would have to pretend
/// a read of such a region means something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl Access {
    pub fn readable(self) -> bool {
        matches!(self, Access::ReadOnly | Access::ReadWrite)
    }

    pub fn writable(self) -> bool {
        matches!(self, Access::WriteOnly | Access::ReadWrite)
    }
}

/// One region a backend exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Chosen by the backend. Nothing above the backend interprets it.
    pub name: String,
    /// In bytes, whatever the addressable unit is.
    pub size: usize,
    pub access: Access,
    /// The addressable unit, where it is not one byte. Video memory addressed
    /// by the word is the usual reason, and it is the difference between an
    /// offset the backend means and an offset the host invents.
    pub unit: usize,
}

impl Region {
    /// A plain byte-addressed region.
    pub fn bytes(name: impl Into<String>, size: usize, access: Access) -> Self {
        Region {
            name: name.into(),
            size,
            access,
            unit: 1,
        }
    }

    /// Checks a span against this region, so that every backend does not have
    /// to get the same arithmetic right separately.
    ///
    /// Overflow is checked rather than assumed: an offset near the top of the
    /// address space plus a length is exactly where a silent wrap would produce
    /// a span that looks valid and reads somewhere else.
    pub fn span(&self, offset: usize, len: usize) -> Result<(), SpanError> {
        let end = offset.checked_add(len).ok_or(SpanError::Overflows {
            region: self.name.clone(),
            offset,
            len,
        })?;
        if end > self.size {
            return Err(SpanError::PastTheEnd {
                region: self.name.clone(),
                offset,
                len,
                size: self.size,
            });
        }
        Ok(())
    }
}

/// Why a span is not a span of this region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanError {
    PastTheEnd {
        region: String,
        offset: usize,
        len: usize,
        size: usize,
    },
    Overflows {
        region: String,
        offset: usize,
        len: usize,
    },
}

impl std::fmt::Display for SpanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpanError::PastTheEnd {
                region,
                offset,
                len,
                size,
            } => write!(
                f,
                "{len} bytes from {offset} run past the end of region `{region}`, which holds {size}"
            ),
            SpanError::Overflows {
                region,
                offset,
                len,
            } => write!(
                f,
                "{len} bytes from {offset} overflow an address in region `{region}`"
            ),
        }
    }
}

impl std::error::Error for SpanError {}

/// The set of regions a backend exposes, as the host sees it.
///
/// It is a list and a lookup, and deliberately not a map keyed by anything the
/// host chose: the order is the backend's, and so are the names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regions {
    regions: Vec<Region>,
}

impl Regions {
    pub fn new(regions: Vec<Region>) -> Self {
        Regions { regions }
    }

    /// The region by that exact name, or `None`.
    ///
    /// Names match exactly, including case. A lookup that normalised would be
    /// deciding something about the backend's naming on the backend's behalf,
    /// and two regions differing only in case would then collide silently.
    pub fn get(&self, name: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Region> {
        self.regions.iter()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.regions.iter().map(|r| r.name.as_str())
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wram() -> Region {
        Region::bytes("wram", 0x2_0000, Access::ReadWrite)
    }

    #[test]
    fn a_span_inside_the_region_is_accepted() {
        let r = wram();
        assert_eq!(r.span(0, 0), Ok(()));
        assert_eq!(r.span(0, r.size), Ok(()), "the whole region is a valid span");
        assert_eq!(r.span(r.size, 0), Ok(()), "an empty span at the end is valid");
    }

    /// The case that matters: one byte too many is refused, not truncated.
    /// A backend that clamped instead would return short data that compares
    /// equal for the wrong reason.
    #[test]
    fn a_span_one_byte_past_the_end_is_refused() {
        let r = wram();
        let err = r.span(0, r.size + 1).expect_err("one byte too many");
        assert!(matches!(err, SpanError::PastTheEnd { .. }));
        assert!(
            err.to_string().contains("wram"),
            "the message must name the region, said: {err}"
        );

        assert!(r.span(r.size, 1).is_err(), "starting at the end reads nothing");
        assert!(r.span(r.size + 1, 0).is_err(), "an empty span past the end is still past it");
    }

    /// An offset and a length that wrap are refused rather than producing a
    /// span that looks valid. Without the checked addition, `offset + len`
    /// wraps to a small number and the span is accepted.
    #[test]
    fn a_span_that_overflows_is_refused() {
        let r = wram();
        let err = r.span(usize::MAX, 1).expect_err("that wraps");
        assert!(
            matches!(err, SpanError::Overflows { .. }),
            "expected an overflow, got {err:?} — which means the addition wrapped silently"
        );
    }

    #[test]
    fn regions_are_found_by_their_exact_name() {
        let set = Regions::new(vec![wram(), Region::bytes("vram", 0x1_0000, Access::ReadWrite)]);
        assert_eq!(set.get("wram").map(|r| r.size), Some(0x2_0000));
        assert_eq!(set.get("vram").map(|r| r.size), Some(0x1_0000));
        assert!(set.get("WRAM").is_none(), "names match exactly, including case");
        assert!(set.get("wra").is_none(), "and are not prefixes");
        assert!(set.get("nothing").is_none());
        assert_eq!(set.names().collect::<Vec<_>>(), ["wram", "vram"], "in the backend's order");
    }

    #[test]
    fn access_answers_both_questions() {
        assert!(Access::ReadOnly.readable() && !Access::ReadOnly.writable());
        assert!(!Access::WriteOnly.readable() && Access::WriteOnly.writable());
        assert!(Access::ReadWrite.readable() && Access::ReadWrite.writable());
    }
}
