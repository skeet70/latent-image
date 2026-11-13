#![forbid(unsafe_code)]

//! X-M1 1.50 addresses of the layer's state blocks and the offsets of their fields.

use crate::controller::Operation;
use crate::memory::{Block, Location};

const SPAN_START: usize = 0x1698a34;
/// The record-mode block, the last, ends at 0x169cefc.
pub const SPAN_BYTES: usize = 0x169cefc - SPAN_START;

pub const fn base(block: Block) -> usize {
    match block {
        Block::Changes => 0x1698a34,
        Block::Controller => 0x1698a50,
        Block::Parameters => 0x169953c,
        Block::Status => 0x1699584,
        Block::Requests => 0x169964c,
        Block::Counters => 0x1699688,
        Block::Work => 0x1699698,
        Block::Selection => 0x1699cf8,
        Block::Policy => 0x1699d78,
        Block::Dump => 0x169b284,
        Block::Diagnostics => 0x169b400,
        Block::Display => 0x169b458,
        Block::GeometryOperation => 0x169b8f0,
        Block::OperationCallbacks => 0x169b950,
        Block::RecordMode => 0x169c1d4,
    }
}

pub const fn address(at: Location) -> usize { base(at.block) + at.offset as usize }

pub const fn span_offset(at: Location) -> usize { address(at) - SPAN_START }

const fn at(block: Block, offset: u16) -> Location { Location::new(block, offset) }

/// The byte counts 0xed87b0 clears.
pub mod cleared {
    pub const CHANGES: u16 = 0x1c;
    pub const PARAMETERS: u16 = 0x48;
    pub const STATUS: u16 = 200;
    pub const REQUESTS: u16 = 0x3c;
    pub const COUNTERS: u16 = 0x10;
    pub const WORK: u16 = 0x660;
    pub const SELECTION: u16 = 0x7e;
    /// 0xedc160.
    pub const POLICY: u16 = 0x14c4;
    /// Each operation's own span, from its state word.
    pub const OPERATIONS: [(super::Operation, u16); 7] = [
        (super::Operation::Primary, 0xfc),
        (super::Operation::First, 0x200),
        (super::Operation::Second, 0x200),
        (super::Operation::Third, 0x1b8),
        (super::Operation::Indexed, 200),
        (super::Operation::Channel, 0x25c),
        (super::Operation::Auxiliary, 0xa8),
    ];
}

pub mod controller {
    use super::at;
    use crate::controller::Operation;
    use crate::memory::{Block::Controller, Location};

    pub const LIFECYCLE: Location = at(Controller, 0x0);

    pub const fn state(operation: Operation) -> Location {
        at(Controller, match operation {
            Operation::Primary => 0x4,
            Operation::First => 0x100,
            Operation::Second => 0x300,
            Operation::Third => 0x500,
            Operation::Indexed => 0x6b8,
            Operation::Auxiliary => 0x7a4,
            Operation::Channel => 0x84c,
        })
    }

    /// The completion the operation's starter stores.
    pub const fn completion(operation: Operation) -> Option<Location> {
        match operation {
            Operation::Primary => Some(at(Controller, 0x8)),
            Operation::First => Some(at(Controller, 0x104)),
            Operation::Second => Some(at(Controller, 0x304)),
            Operation::Third => Some(at(Controller, 0x504)),
            Operation::Indexed => Some(at(Controller, 0x6bc)),
            Operation::Auxiliary => Some(at(Controller, 0x7a8)),
            Operation::Channel => None,
        }
    }

    /// The stages the first operation runs, which stopping it stops.
    pub const FIRST_STAGES: Location = at(Controller, 0x2fc);
    /// Non-zero while the primary operation's completions are delivered (0xed84cc).
    pub const PRIMARY_COMPLETIONS: Location = at(Controller, 0xfc);
    /// Set by the primary operation's starter; while it is 1 the first operation and the format
    /// work wait for the primary operation to end (0xee3514, 0xee5aa4).
    pub const PRIMARY_FLAG: Location = at(Controller, 0xf4);
    /// Non-zero holds the primary operation's flag (0xee345c).
    pub const PRIMARY_HOLD: Location = at(Controller, 0x7a0);
    pub const CHANNEL_TOGGLE: Location = at(Controller, 0xaa0);
    pub const PIPELINE_CHANNEL: Location = at(Controller, 0xaa8);
    pub const UNIT_HANDLES: [Location; 2] = [at(Controller, 0xab4), at(Controller, 0xab8)];
    pub const CHANNEL_HELD: Location = at(Controller, 0xabc);
    pub const START_CLEARED: [Location; 3] = [at(Controller, 0xac0), at(Controller, 0xac4), at(Controller, 0xad4)];
    pub const SELECTED: Location = at(Controller, 0xacc);
    pub const RECORD_KIND: Location = at(Controller, 0xad0);
    /// Non-zero while a configured profile kind of zero takes its geometry from the class
    /// remainders instead of the profile class (0xedcc60, 0xee2d28).
    pub const REMAINDER_GEOMETRY: Location = at(Controller, 0xad4);
    pub const INITIALIZED_CLEARED: Location = at(Controller, 0xae4);
    pub const SCAN_OPTION: Location = at(Controller, 0xae8);
    /// The crop alignment mode the primary operation's starter stores.
    pub const CROP_MODE: Location = at(Controller, 0xb0);

    /// The crop the primary operation's starter stores, which the refresh aligns in place.
    pub mod crop {
        use super::at;
        use crate::memory::{Block::Controller, Location};

        pub const TOP: Location = at(Controller, 0xbc);
        pub const LEFT: Location = at(Controller, 0xc0);
        pub const WIDTH: Location = at(Controller, 0xc4);
        pub const HEIGHT: Location = at(Controller, 0xc8);
        /// The colour-pattern phase of the aligned top and left.
        pub const PHASE: [Location; 2] = [at(Controller, 0xd0), at(Controller, 0xd1)];
        /// What aligning the top to the magnification removed.
        pub const TOP_EXCESS: Location = at(Controller, 0xd4);
        pub const FORMAT: Location = at(Controller, 0xdc);
        pub const SCALE_MODE: Location = at(Controller, 0xe0);
    }

    /// The third operation's starter fields each record grid is computed from.
    pub mod grids {
        use super::at;
        use crate::memory::{Block::Controller, Location};

        pub const COMPACT: Location = at(Controller, 0x508);
        pub const EXTENDED: Location = at(Controller, 0x514);
        pub const RECORD: Location = at(Controller, 0x558);
    }

    /// The indexed operation's window: top, height, left and width.
    pub const INDEXED_PAIR: Location = at(Controller, 0x6c4);
}

pub mod changes {
    use super::at;
    use crate::controller::changes::{Hold, Stage, Work};
    use crate::memory::{Block::Changes, Location};

    pub const fn work(work: Work) -> (Location, u8) {
        let (byte, bit) = match work {
            Work::Output => (0x0, 0x01),
            Work::Crop => (0x0, 0x02),
            Work::PendingPair => (0x0, 0x04),
            Work::PrimaryPayload => (0x2, 0x02),
            Work::SecondaryPayload => (0x2, 0x04),
            Work::OutputCompanion => (0x2, 0x08),
            Work::FormatCompanion => (0x2, 0x10),
            Work::Format => (0x2, 0x20),
            Work::QuadSelection => (0x2, 0x40),
            Work::Mode => (0x2, 0x80),
            Work::ScanConfiguration => (0x3, 0x01),
            Work::Tertiary => (0x3, 0x02),
            Work::PrimaryRectangles => (0x4, 0x02),
            Work::SecondaryRectangles => (0x4, 0x04),
            Work::PrimaryScan => (0x4, 0x08),
            Work::SecondaryScan => (0x4, 0x10),
            Work::FourthCompanion => (0x8, 0x01),
            Work::ExtendedRectangles => (0x8, 0x02),
            Work::CompactRectangles => (0x8, 0x04),
            Work::RectangleSet => (0x8, 0x40),
            Work::RecordRectangles => (0x8, 0x80),
            Work::IndexedPair => (0x9, 0x01),
        };
        (at(Changes, byte), bit)
    }

    pub const fn claim(stage: Stage) -> (Location, u8) {
        let (byte, bit) = match stage {
            Stage::PrimaryPayload => (0xc, 0x01),
            Stage::SecondaryPayload => (0xc, 0x02),
            Stage::Base => (0xc, 0x04),
            Stage::Format => (0xc, 0x08),
            Stage::QuadSelection => (0xc, 0x10),
            Stage::FirstMode => (0xc, 0x20),
            Stage::SecondMode => (0xc, 0x40),
            Stage::PrimaryScan => (0xd, 0x01),
            Stage::SecondaryScan => (0xd, 0x02),
            Stage::Tertiary => (0xd, 0x04),
        };
        (at(Changes, byte), bit)
    }

    pub const SCAN_CLAIM: (Location, u8) = (at(Changes, 0xc), 0x80);

    pub const fn pending(stage: Stage) -> (Location, u8) {
        let (byte, bit) = match stage {
            Stage::PrimaryPayload => (0x10, 0x01),
            Stage::SecondaryPayload => (0x10, 0x02),
            Stage::Base => (0x10, 0x04),
            Stage::Format => (0x10, 0x08),
            Stage::QuadSelection => (0x10, 0x10),
            Stage::FirstMode => (0x10, 0x40),
            Stage::SecondMode => (0x10, 0x80),
            Stage::PrimaryScan => (0x11, 0x01),
            Stage::SecondaryScan => (0x11, 0x02),
            Stage::Tertiary => (0x11, 0x04),
        };
        (at(Changes, byte), bit)
    }

    /// The pending bit the quad selection clears beside its stage's (0xee6414).
    pub const QUAD_SELECTION_CLEARED: (Location, u8) = (at(Changes, 0x10), 0x20);

    pub const fn hold(hold: Hold) -> (Location, Location, u8) {
        let bit = match hold {
            Hold::PrimaryPayload => 0x01,
            Hold::SecondaryPayload => 0x02,
            Hold::QuadSelection => 0x08,
            Hold::Mode => 0x10,
            Hold::Tertiary => 0x20,
        };
        (at(Changes, 0x14), at(Changes, 0x18), bit)
    }
}

pub mod requests {
    use super::at;
    use crate::controller::changes::Subscription;
    use crate::memory::{Block::Requests, Location};

    pub const fn subscription(subscription: Subscription) -> Location {
        let index = match subscription {
            Subscription::Output => 0,
            Subscription::Format => 1,
            Subscription::PrimaryScan => 2,
            Subscription::SecondaryScan => 3,
            Subscription::Fourth => 4,
            Subscription::ScanConfiguration => 5,
            Subscription::Grouped(index) => index as u16,
        };
        at(Requests, index * 4)
    }
}

pub mod work {
    use super::at;
    use crate::controller::changes::Mask;
    use crate::memory::{Block::Work, Location};

    pub const fn mask(mask: Mask) -> Location {
        at(Work, match mask {
            Mask::Geometry => 0x0,
            Mask::Apply => 0x4,
            Mask::Enable => 0x10,
            Mask::Disable => 0x14,
        })
    }

    /// The geometry records the geometry mask names; the crop record is first.
    pub const GEOMETRY_RECORDS: Location = at(Work, 0x20);
    /// Zeroed by a pending-pair reset.
    pub const PENDING_PAIR: [Location; 2] = [at(Work, 0xb0), at(Work, 0xb2)];
    /// The payloads the enable and apply masks name.
    pub const PAYLOADS: Location = at(Work, 0x370);
    pub const SECONDARY_PAYLOAD_OFFSET: Location = at(Work, 0x3ae);
    /// Payload origins the alternate geometry moves by its offset pair, by apply stage.
    pub const OFFSET_PAYLOADS: [(u32, (Location, Location)); 3] = [
        (0x20, (at(Work, 0x5c8), at(Work, 0x5ca))),
        (0x40, (at(Work, 0x3b4), at(Work, 0x3b6))),
        (0x80, (at(Work, 0x3b8), at(Work, 0x3ba))),
    ];
    /// Payload origins the alternate geometry moves by twice its offset pair.
    pub const DOUBLED_OFFSET_PAYLOADS: [(u32, (Location, Location)); 2] =
        [(0x100, (at(Work, 0x4a8), at(Work, 0x4ac))), (0x200, (at(Work, 0x538), at(Work, 0x53c)))];
    /// Set to 1 when the pipeline accepts an apply naming the stage.
    pub const APPLIED: [(u32, Location); 3] = [(0x80, at(Work, 0x3f4)), (0x20, at(Work, 0x600)), (0x4, at(Work, 0x3a4))];
    pub const MODE_PAYLOADS: Location = at(Work, 0x3b4);
    pub const SECOND_MODE_CLEARED: [Location; 2] = [at(Work, 0x3ec), at(Work, 0x3f0)];
    pub const QUAD_SELECTION: Location = at(Work, 0x3f8);
    /// Top, left, height and width of the area quad selection.
    pub const QUAD_AREA: [Location; 4] = [at(Work, 0x3fc), at(Work, 0x3fe), at(Work, 0x400), at(Work, 0x402)];
    pub const QUAD_AREA_SCALE: Location = at(Work, 0x404);
    pub const QUAD_AREA_LIMIT: Location = at(Work, 0x408);
    /// The four values the quad selection keeps (0xee6414).
    pub const QUAD_GAINS: [Location; 4] = [at(Work, 0x40a), at(Work, 0x40c), at(Work, 0x40e), at(Work, 0x410)];
    pub const TERTIARY_PAYLOAD: Location = at(Work, 0x5c4);
    pub const TERTIARY_CLEARED: [Location; 2] = [at(Work, 0x5fc), at(Work, 0x5f8)];
    /// The adjusted triplet the refresh keeps (0xee13e0, 0xee20a0).
    pub const WHITE_BALANCE: [Location; 3] = [at(Work, 0x648), at(Work, 0x64a), at(Work, 0x64c)];
    /// 1 after the scan configuration was rebuilt, until the policy stages are restored.
    pub const SCAN_REBUILT: Location = at(Work, 0x650);

    /// The geometry records each geometry stage sends.
    pub mod records {
        use super::at;
        use crate::memory::{Block::Work, Location};

        pub const PRIMARY_SCAN: Location = at(Work, 0x28);
        pub const SECONDARY_SCAN: Location = at(Work, 0x60);
        pub const COMPACT_GRID: Location = at(Work, 0xb4);
        /// Ten row spans, then ten column spans.
        pub const FIXED_WINDOWS: Location = at(Work, 0xc4);
        pub const EXTENDED_GRID: Location = at(Work, 0x114);
        /// As `FIXED_WINDOWS`.
        pub const TRACKED_WINDOWS: Location = at(Work, 0x124);
        pub const RECORD_GRID: Location = at(Work, 0x174);
        /// A plain, a row-banded and a column-banded rectangle.
        pub const RECTANGLE_SET: Location = at(Work, 0x184);
        pub const ROW_BANDS: Location = at(Work, 0x18c);
        pub const COLUMN_BANDS: Location = at(Work, 0x196);
        pub const INDEXED_PAIR: Location = at(Work, 0x1a0);
    }

    /// The scan stages' radial tables: gains, the scan record's count and base, bounds, scales
    /// and 31 points.
    pub mod scan_geometry {
        use super::at;
        use crate::memory::{Block::Work, Location};

        pub const fn base(secondary: bool) -> Location { at(Work, if secondary { 0x534 } else { 0x4a4 }) }
    }

    /// The primary and secondary payloads within the payload table.
    pub mod payloads {
        use super::at;
        use crate::memory::{Block::Work, Location};

        pub const PRIMARY_ENTRIES: Location = at(Work, 0x388);
        pub const PRIMARY_COUNT: Location = at(Work, 0x38c);
        pub const PRIMARY_OFFSETS: [Location; 2] = [at(Work, 0x38e), at(Work, 0x390)];
        pub const PRIMARY_UPDATED: Location = at(Work, 0x394);
        pub const PRIMARY_UPDATE_CLEARED: Location = at(Work, 0x398);
        pub const PRIMARY_OPTION: Location = at(Work, 0x3a0);
        pub const SECONDARY_VALUE: Location = at(Work, 0x3a8);
        pub const SECONDARY_PAIR: [Location; 2] = [at(Work, 0x3ac), at(Work, 0x3ae)];
        pub const SECONDARY_EXTRA: Location = at(Work, 0x3b0);
    }
}

pub mod parameters {
    use super::at;
    use crate::memory::{Block::Parameters, Location};

    pub const PROFILE: Location = at(Parameters, 0x0);
    pub const LIMIT: Location = at(Parameters, 0x4);
    pub const SECONDARY_LIMIT: Location = at(Parameters, 0x6);
    pub const INDEX: Location = at(Parameters, 0x8);
    pub const INDEX_SCALE: Location = at(Parameters, 0xc);
    /// Three words; the first is the diagnostic value.
    pub const AREA: Location = at(Parameters, 0x10);
    pub const MODE_SHIFT: Location = at(Parameters, 0x1c);
    /// 3 and 4 after initialization.
    pub const FIRST_DIVISOR: Location = at(Parameters, 0x1e);
    pub const SECOND_DIVISOR: Location = at(Parameters, 0x20);
    pub const QUAD_MODE: Location = at(Parameters, 0x24);
    pub const SCAN_FIRST: Location = at(Parameters, 0x28);
    pub const SCAN_SECOND: Location = at(Parameters, 0x2a);
    /// Kept only for profiles of the extended classes.
    pub const EXTENDED_PAIR: Location = at(Parameters, 0x30);
    pub const MODE_WORD: Location = at(Parameters, 0x38);
    pub const MODE_FIRST: Location = at(Parameters, 0x3c);
    pub const MODE_SECOND: Location = at(Parameters, 0x40);
    pub const MODE_BYTE: Location = at(Parameters, 0x44);
}

pub mod status {
    use super::at;
    use crate::controller::refresh::StatusEffect::{self, Plain, ScanConfiguration};
    use crate::memory::{Block::Status, Location};

    pub const AREA: Location = at(Status, 0x0);
    pub const OUTPUT_MODE: Location = at(Status, 0xc);
    pub const SCAN_PAIR: Location = at(Status, 0x1c);
    pub const PRIMARY_PAIR: Location = at(Status, 0x2c);
    pub const SECONDARY_PAIR: Location = at(Status, 0x34);
    /// Five bytes: a word and a halfword.
    pub const TRIPLET: Location = at(Status, 0x3c);
    pub const TRACKING_ARMED: Location = at(Status, 0x8c);
    pub const TRACKED_INDEX: Location = at(Status, 0x90);
    pub const SCAN_PROFILE: Location = at(Status, 0x94);
    pub const TRACKING_HELD: Location = at(Status, 0x98);
    pub const RECORD_KIND: Location = at(Status, 0x9c);
    pub const RATE: Location = at(Status, 0xa2);
    pub const SCAN_INDEX: Location = at(Status, 0xc0);
    pub const SCAN_LIMIT: Location = at(Status, 0xc4);

    /// The single fields the snapshot refresh copies (0xee1d94..0xee2aa8), in its order.
    pub const FIELDS: [(u16, u8, StatusEffect); 29] = [
        (0x0c, 4, Plain),
        (0x10, 4, ScanConfiguration),
        (0xc4, 2, Plain),
        (0x14, 4, ScanConfiguration),
        (0x18, 4, ScanConfiguration),
        (0x44, 4, ScanConfiguration),
        (0x74, 4, ScanConfiguration),
        (0x48, 4, ScanConfiguration),
        (0x4c, 4, Plain),
        (0x50, 4, Plain),
        (0x5c, 4, ScanConfiguration),
        (0x60, 4, Plain),
        (0x64, 4, ScanConfiguration),
        (0x68, 4, ScanConfiguration),
        (0x6c, 4, ScanConfiguration),
        (0x70, 1, ScanConfiguration),
        (0x78, 2, ScanConfiguration),
        (0x9c, 4, ScanConfiguration),
        (0x54, 4, ScanConfiguration),
        (0xa0, 1, Plain),
        (0xa2, 2, StatusEffect::Rate),
        (0xa4, 4, ScanConfiguration),
        (0xa8, 4, ScanConfiguration),
        (0xac, 4, ScanConfiguration),
        (0xb0, 4, ScanConfiguration),
        (0xbc, 1, ScanConfiguration),
        (0xb4, 4, ScanConfiguration),
        (0xb8, 4, ScanConfiguration),
        (0xc0, 4, Plain),
    ];
}

pub mod counters {
    use super::at;
    use crate::memory::{Block::Counters, Location};

    /// Non-zero while the tracked index follows the snapshot one step per frame.
    pub const TRACKING: Location = at(Counters, 0x0);
    pub const INDEX: Location = at(Counters, 0x4);
}

pub mod dump {
    use super::at;
    use crate::memory::{Block::Dump, Location};

    /// The buffer addresses 0xedf558 stores, in its order.
    pub const BUFFERS: [(Location, u32); 8] = [
        (at(Dump, 0x10), 0x5ffb0dc0),
        (at(Dump, 0x14), 0x5ffb3770),
        (at(Dump, 0xc), 0x5ffb9120),
        (at(Dump, 0x0), 0x5ffbbad0),
        (at(Dump, 0x8), 0x5ffbe480),
        (at(Dump, 0x4), 0x5ffc0e30),
        (at(Dump, 0x18), 0x5ffc3c00),
        (at(Dump, 0x1c), 0x5ffc3c10),
    ];
    pub const REGIONS: [(u32, u32); 2] = [(12, 0x5ffb0dc0), (13, 0x5ffb3770)];
    pub const REGION_BYTES: u32 = 0x29b0;
}

pub mod policy {
    use super::at;
    use crate::memory::{Block::Policy, Location};

    /// The configuration handed to the pipeline; its first word is zero while none is held.
    pub const CONFIGURATION: Location = at(Policy, 0x258);
    pub const CONFIGURATION_BYTES: u16 = 0x1260;
    /// The stages the scan configuration hands back once rebuilt.
    pub const RESTORED_STAGES: Location = at(Policy, 0x160);
    /// The scan slot the last scan record was computed for.
    pub const SLOT: Location = at(Policy, 0x0);
    /// The status pair each scan record was computed from, primary then secondary.
    pub const SCAN_PAIRS: [Location; 2] = [at(Policy, 0x4), at(Policy, 0xc)];
    /// The part of the block the frame dumps copy.
    pub const DUMPED: Location = at(Policy, 0x14);
    pub const LAYOUT: Location = at(Policy, 0x1284);
    /// Cleared by every scan record.
    pub const SCAN_CLEARED: Location = at(Policy, 0x14b8);
    pub const SCAN_CLEARED_HALF: Location = at(Policy, 0x14c0);
}

pub mod selection {
    use super::at;
    use crate::memory::{Block::Selection, Location};

    /// The callbacks the scan configuration copies into its compiled form.
    pub const BLOCK: Location = at(Selection, 0x0);
}

pub mod callbacks {
    use super::at;
    use crate::memory::{Block::OperationCallbacks, Location};

    /// 1 while operation callbacks are enabled (0xeef1c0).
    pub const ENABLED: Location = at(OperationCallbacks, 0x0);
}

pub mod diagnostics {
    use super::at;
    use crate::memory::{Block::Diagnostics, Location};

    pub const ENABLED: Location = at(Diagnostics, 0x0);
    pub const DESCRIPTION: Location = at(Diagnostics, 0x9);
    pub const DESCRIPTION_BYTES: usize = 0x14;
    pub const INDEX: Location = at(Diagnostics, 0x1e);
    pub const INDEX_VALUE: Location = at(Diagnostics, 0x20);
    pub const LIMIT: Location = at(Diagnostics, 0x22);
    pub const SECONDARY_LIMIT: Location = at(Diagnostics, 0x24);
    pub const WHITE_BALANCE: [Location; 2] = [at(Diagnostics, 0x2c), at(Diagnostics, 0x30)];
    pub const GAINS: [Location; 2] = [at(Diagnostics, 0x34), at(Diagnostics, 0x38)];
    pub const RATE: Location = at(Diagnostics, 0x3c);
    pub const VALUE: Location = at(Diagnostics, 0x40);
    /// Non-zero shows the fixed label instead of the description.
    pub const FIXED: Location = at(Diagnostics, 0x48);
}
