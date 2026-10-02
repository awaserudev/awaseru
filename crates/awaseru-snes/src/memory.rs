//! Which of the backend's memories become regions, and under what names.
//!
//! The backend enumerates every memory of every console it supports in one
//! list, and addresses them by position in that list. This module is the only
//! place that knows those positions, and the only place that chooses names for
//! them.
//!
//! # Where the positions come from
//!
//! Counted from the backend's own enumeration — and then **measured**, which is
//! the part that matters. Asking a loaded backend for the size of each position
//! and checking it against what the hardware is known to hold is a check the
//! counting cannot pass by accident: a transcription off by one would report a
//! region of the wrong size, or of none.
//!
//! The measurement that fixed these values, against a backend at version 2.2.1
//! with a cartridge loaded:
//!
//! | position | size reported | what holds that much |
//! |---|---|---|
//! | 14 | 2 MiB | the cartridge's program data, for that cartridge |
//! | 15 | 128 KiB | the console's own work memory |
//! | 16 | 8 KiB | the cartridge's battery-backed memory, for that cartridge |
//! | 17 | 64 KiB | the video memory |
//! | 18 | 544 bytes | the sprite table |
//! | 19 | 512 bytes | the palette |
//!
//! The four sizes that are properties of the *console* rather than of the
//! cartridge — 128 KiB, 64 KiB, 544 and 512 — are the ones that make this a
//! check rather than a coincidence.
//!
//! # What is deliberately absent
//!
//! Two of the backend's memories are not here, and their absence is a decision
//! (§2.4), not an omission:
//!
//! - **The processor's address space** (position 0, 16 MiB). Reading it is not
//!   reading storage: it goes through the memory map, so a read can land on a
//!   hardware register and observe, or disturb, something. A region whose read
//!   is not a read of bytes does not belong in a model whose whole purpose is
//!   comparing bytes.
//! - **The hardware registers** (position 20). Whether what the backend returns
//!   for them is the value last written, the value a read would return, or
//!   something else again has not been established. §3.1 has `WriteOnly` for
//!   exactly this case and it can be used the moment the question is answered;
//!   until then, claiming they are readable would be claiming something
//!   unverified.

use awaseru_core::Access;

/// One of the backend's memories, and what this crate calls it.
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    /// The name the host will use. Chosen here; nothing above interprets it
    /// (§2.7).
    pub region: &'static str,
    /// The memory's position in the backend's enumeration.
    pub memory_type: u32,
    pub access: Access,
    /// The backend's own word for it, kept so that someone holding the
    /// backend's source can check this table against it without guessing which
    /// row is which.
    pub backend_calls_it: &'static str,
}

/// Every memory this backend exposes that is modelled as a region.
///
/// The order is the backend's, which is the order the host sees (§3.1).
pub const MAPPINGS: &[Mapping] = &[
    Mapping {
        region: "program-rom",
        memory_type: 14,
        // The cartridge's program data. Writable in the backend's debugger, but
        // not writable by the console, and a comparison that wrote to it would
        // be changing the subject rather than measuring it.
        access: Access::ReadOnly,
        backend_calls_it: "SnesPrgRom",
    },
    Mapping {
        region: "work-ram",
        memory_type: 15,
        access: Access::ReadWrite,
        backend_calls_it: "SnesWorkRam",
    },
    Mapping {
        region: "save-ram",
        memory_type: 16,
        access: Access::ReadWrite,
        backend_calls_it: "SnesSaveRam",
    },
    Mapping {
        region: "video-ram",
        memory_type: 17,
        access: Access::ReadWrite,
        backend_calls_it: "SnesVideoRam",
    },
    Mapping {
        region: "sprite-ram",
        memory_type: 18,
        access: Access::ReadWrite,
        backend_calls_it: "SnesSpriteRam",
    },
    Mapping {
        region: "palette-ram",
        memory_type: 19,
        access: Access::ReadWrite,
        // The backend's name says colour-generator, which is what the hardware
        // documentation calls it; what it holds is the palette.
        backend_calls_it: "SnesCgRam",
    },
];

/// Every memory this backend exposes that can be written and is not read-only,
/// with a name for a report to use.
///
/// **This set is larger than `MAPPINGS`, and that is the point.** Two of these
/// are not modelled as regions at all — the sound processor's memory and its
/// registers — because nothing compares them yet. But determinism does not care
/// what this project models: leaving either of them random leaves a run that
/// does not repeat, and the sound processor's registers were the **last** thing
/// still differing between processes when the other six had been zeroed
/// (`doc/backend.md`).
///
/// So the two lists are kept apart on purpose. One is "what a comparison can be
/// about"; this one is "what has to be settled for a comparison to mean
/// anything".
pub const ZEROED_AT_POWER_ON: &[(u32, &str)] = &[
    (15, "work-ram"),
    (16, "save-ram"),
    (17, "video-ram"),
    (18, "sprite-ram"),
    (19, "palette-ram"),
    (21, "sound-ram"),
    (23, "sound-registers"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every other test in this module iterates the table, so an empty table
    /// would make all of them pass while testing nothing. This is the one that
    /// makes them not vacuous.
    #[test]
    fn the_table_is_not_empty() {
        assert!(
            MAPPINGS.len() >= 4,
            "the table has {} entries; the console's work memory, its video memory, its sprite              table and its palette are the four a comparison cannot do without",
            MAPPINGS.len()
        );
    }

    /// Two mappings pointing at one memory, or two names for one region, would
    /// both be transcription slips that nothing else would catch: the second
    /// would simply shadow the first at lookup.
    #[test]
    fn no_region_name_and_no_memory_position_is_used_twice() {
        for (i, a) in MAPPINGS.iter().enumerate() {
            for b in &MAPPINGS[i + 1..] {
                assert_ne!(a.region, b.region, "two mappings share a region name");
                assert_ne!(
                    a.memory_type, b.memory_type,
                    "`{}` and `{}` both claim position {}",
                    a.region, b.region, a.memory_type
                );
            }
        }
    }

    /// No name here may name the console, its maker, or anything a title is
    /// about. The names cross into the host and appear in its output (§2.7,
    /// §11.2), and a platform name in one of them is how a tool that claims to
    /// be platform-independent stops being it.
    #[test]
    fn no_region_name_carries_a_platform_name() {
        for mapping in MAPPINGS {
            let name = mapping.region;
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "`{name}` should be lower-case words joined by hyphens"
            );
            for forbidden in ["snes", "nes", "sfc", "super", "nintendo", "mesen", "spc"] {
                assert!(
                    !name.split('-').any(|word| word == forbidden),
                    "the region name `{name}` carries `{forbidden}`, which names a platform"
                );
            }
        }
    }

    /// Everything this crate models as a writable region is in the set that
    /// gets zeroed. A region modelled and left random would be a region whose
    /// comparisons do not repeat.
    #[test]
    fn every_writable_mapping_is_also_zeroed_at_power_on() {
        for mapping in MAPPINGS.iter().filter(|m| m.access.writable()) {
            assert!(
                ZEROED_AT_POWER_ON
                    .iter()
                    .any(|(t, _)| *t == mapping.memory_type),
                "`{}` is writable and not in the set zeroed at power-on, so a run over it \
                 would not repeat",
                mapping.region
            );
        }
    }

    /// And the set is larger than the mappings, which is deliberate: the two
    /// memories nothing models yet still have to be settled. A test, because
    /// somebody tidying the two lists into one would break determinism and
    /// nothing else would notice.
    #[test]
    fn the_zeroed_set_reaches_memories_nothing_models() {
        let unmodelled: Vec<&str> = ZEROED_AT_POWER_ON
            .iter()
            .filter(|(t, _)| !MAPPINGS.iter().any(|m| m.memory_type == *t))
            .map(|(_, name)| *name)
            .collect();
        assert_eq!(
            unmodelled,
            ["sound-ram", "sound-registers"],
            "the sound processor's memory and registers are not regions and must still be \
             zeroed; the second was the last thing differing between processes"
        );
        for (_, name) in ZEROED_AT_POWER_ON {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "`{name}` should be lower-case words joined by hyphens"
            );
        }
    }

    /// Only one region is read-only, and it is the cartridge's own program
    /// data. This is here because `Access` is easy to fill in by habit, and a
    /// region wrongly marked read-only cannot be seeded into later.
    #[test]
    fn only_the_cartridge_program_is_read_only() {
        let read_only: Vec<&str> = MAPPINGS
            .iter()
            .filter(|m| !m.access.writable())
            .map(|m| m.region)
            .collect();
        assert_eq!(read_only, ["program-rom"]);
        assert!(
            MAPPINGS.iter().all(|m| m.access.readable()),
            "a region nothing can read would have nothing to compare"
        );
    }
}
