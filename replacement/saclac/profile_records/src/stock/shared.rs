#![forbid(unsafe_code)]

//! The shared-block fields task 43's own accessors reach, by the group their writes are granted
//! under, with the accessors each one replaces. `AT_` names are offsets whose meaning no caller
//! has shown yet.

use crate::field::{SharedArray, SharedField, SharedRead, SharedWrite, Unread, Unwritten};
use crate::handle::AccessGroup;

/// Read without a gate; no task-43 accessor writes them.
pub mod read_only {
    use super::*;

    /// Read by 0x757a08.
    pub const AT_026C: SharedField<[u8; 1044], SharedRead, Unwritten> = SharedField::new(0x26c, SharedRead::Open, Unwritten);

    /// Read by 0x757a74.
    pub const AT_0680: SharedField<[u8; 40], SharedRead, Unwritten> = SharedField::new(0x680, SharedRead::Open, Unwritten);

    /// Read by 0x757b0c.
    pub const AT_06A8: SharedField<[u8; 80], SharedRead, Unwritten> = SharedField::new(0x6a8, SharedRead::Open, Unwritten);
}

/// Access group 14. The focus result.
pub mod group_14 {
    use super::*;

    /// The focus result (ShotSequence `FocusResult`).
    /// Read by 0x756734; written by 0x756754.
    pub const FOCUS_RESULT: SharedField<[u8; 132]> = SharedField::grouped(0x0, 14);

    /// Read by 0x757ff0; written by 0x758028.
    pub const AT_0864: SharedField<[u8; 48]> = SharedField::grouped(0x864, 14);

    /// Read by 0x758340; written by 0x758360.
    pub const AT_0890: SharedField<u32> = SharedField::grouped(0x890, 14);
}

/// Access group 15. The focus confirmation.
pub mod group_15 {
    use super::*;

    /// ShotSequence `FocusConfirmation`.
    /// Read by 0x756788; written by 0x7567b8.
    pub const FOCUS_CONFIRMATION: SharedField<[u8; 32]> = SharedField::grouped(0x84, 15);

    /// Read by 0x758068; written by 0x758098.
    pub const AT_074C: SharedField<[u8; 24]> = SharedField::grouped(0x74c, 15);
}

/// Access group 16. The area-statistics descriptor.
pub mod group_16 {
    use super::*;

    /// The area-statistics descriptor (ShotSequence `AreaStatistics`).
    /// Read by 0x7567ec; written by 0x756814.
    pub const AREA_STATISTICS: SharedField<[u8; 12]> = SharedField::grouped(0xa4, 16);
}

/// Access group 17. The lens snapshot.
pub mod group_17 {
    use super::*;

    /// ShotSequence `LensSnapshot`.
    /// Read by 0x756840; written by 0x756878.
    pub const LENS_SNAPSHOT: SharedField<[u8; 48]> = SharedField::grouped(0xb0, 17);
}

/// Access group 18.
pub mod group_18 {
    use super::*;

    /// Read by 0x7568b8; written by 0x7568f8.
    pub const AT_00E0: SharedField<[u8; 64]> = SharedField::grouped(0xe0, 18);
}

/// Access group 19.
pub mod group_19 {
    use super::*;

    /// Read by 0x756940; written by 0x756970.
    pub const AT_0120: SharedField<[u8; 28]> = SharedField::grouped(0x120, 19);
}

/// Access group 20. The measurement round's offers, statistics, capture buffer and image callback.
pub mod group_20 {
    use super::*;

    /// The measurement kinds offered; 0 offers none (FocusRound `offered`).
    /// Read by 0x7569a4; written by 0x7569c4.
    pub const OFFERED_MEASUREMENTS: SharedField<u32> = SharedField::grouped(0x13c, 20);

    /// Read by 0x7569ec; written by 0x756a0c.
    pub const AT_0140: SharedField<u32> = SharedField::grouped(0x140, 20);

    /// Read by 0x756a34; written by 0x756a54.
    pub const AT_0144: SharedField<u32> = SharedField::grouped(0x144, 20);

    /// Read by 0x756a7c; written by 0x756a9c.
    pub const AT_0148: SharedField<u32> = SharedField::grouped(0x148, 20);

    /// Read by 0x756ac4; written by 0x756ae4.
    pub const AT_014C: SharedField<u32> = SharedField::grouped(0x14c, 20);

    /// FocusRound `StatisticsInUse`: bit 0 window, bit 1 image job.
    /// Read by 0x756b0c; written by 0x756b2c.
    pub const STATISTICS_IN_USE: SharedField<u32> = SharedField::grouped(0x150, 20);

    /// The buffer measurement captures go to; the round clears it on reset.
    /// Read by 0x756b54; written by 0x756b74.
    pub const CAPTURE_BUFFER: SharedField<u32> = SharedField::grouped(0x154, 20);

    /// Read by 0x756b9c; written by 0x756bbc.
    pub const AT_0158: SharedField<u32> = SharedField::grouped(0x158, 20);

    /// Written by 0x756c04.
    pub const AT_015C: SharedField<u32, Unread, SharedWrite> = SharedField::new(0x15c, Unread, SharedWrite::Granted(AccessGroup::at(20)));

    /// Written by 0x756c4c.
    pub const AT_0160: SharedField<u32, Unread, SharedWrite> = SharedField::new(0x160, Unread, SharedWrite::Granted(AccessGroup::at(20)));

    /// The image job's record callback (FocusRound).
    /// Read by 0x756c74; written by 0x756c94.
    pub const IMAGE_CALLBACK: SharedField<u32> = SharedField::grouped(0x164, 20);

    /// Read by 0x756d04; written by 0x756d24.
    pub const AT_016C: SharedField<u32> = SharedField::grouped(0x16c, 20);

    /// Read by 0x756d4c; written by 0x756d6c.
    pub const AT_0170: SharedField<u32> = SharedField::grouped(0x170, 20);

    /// Written by 0x756db4.
    pub const AT_0174: SharedField<u32, Unread, SharedWrite> = SharedField::new(0x174, Unread, SharedWrite::Granted(AccessGroup::at(20)));
}

/// Access group 21.
pub mod group_21 {
    use super::*;

    /// Read by 0x756ddc; written by 0x756dfc.
    pub const AT_0178: SharedField<u32> = SharedField::grouped(0x178, 21);
}

/// Access group 22.
pub mod group_22 {
    use super::*;

    /// Read by 0x756e24; written by 0x756e5c.
    pub const AT_0180: SharedField<[u8; 20]> = SharedField::grouped(0x180, 22);
}

/// Access group 23.
pub mod group_23 {
    use super::*;

    /// Read by 0x756e90; written by 0x756ebc.
    pub const AT_0194: SharedField<[u8; 16]> = SharedField::grouped(0x194, 23);
}

/// Access group 24.
pub mod group_24 {
    use super::*;

    /// Written by 0x756f10.
    pub const AT_01A4: SharedField<[u8; 8], Unread, SharedWrite> = SharedField::new(0x1a4, Unread, SharedWrite::Granted(AccessGroup::at(24)));
}

/// Access group 25. The focus history.
pub mod group_25 {
    use super::*;

    /// ShotSequence `FocusHistory`.
    /// Read by 0x756f3c; written by 0x756f60.
    pub const FOCUS_HISTORY: SharedField<[u8; 80]> = SharedField::grouped(0x1ac, 25);
}

/// Access group 26.
pub mod group_26 {
    use super::*;

    /// Read by 0x756f94; written by 0x756fb8.
    pub const AT_01FC: SharedField<i16> = SharedField::grouped(0x1fc, 26);

    /// Read by 0x756fe4; written by 0x757004.
    pub const AT_0200: SharedField<u32> = SharedField::grouped(0x200, 26);

    /// Written by 0x7570f0.
    pub const AT_0204: SharedField<u16, Unread, SharedWrite> = SharedField::new(0x204, Unread, SharedWrite::Granted(AccessGroup::at(26)));

    /// Read by 0x75702c; written by 0x757050.
    pub const AT_0206: SharedField<i16> = SharedField::grouped(0x206, 26);

    /// Read by 0x75707c; written by 0x7570a0.
    pub const AT_0208: SharedField<i16> = SharedField::grouped(0x208, 26);

    /// Read by 0x75711c; written by 0x757140.
    pub const AT_020C: SharedField<i16> = SharedField::grouped(0x20c, 26);

    /// Written by 0x757190.
    pub const AT_0212: SharedField<u16, Unread, SharedWrite> = SharedField::new(0x212, Unread, SharedWrite::Granted(AccessGroup::at(26)));

    /// Read by 0x7571bc; written by 0x7571e0.
    pub const AT_0216: SharedField<i16> = SharedField::grouped(0x216, 26);

    /// Read by 0x75720c; written by 0x75722c.
    pub const AT_0218: SharedField<u32> = SharedField::grouped(0x218, 26);

    /// Read by 0x757254; written by 0x757274.
    pub const AT_0230: SharedField<u32> = SharedField::grouped(0x230, 26);

    /// Read by 0x75733c; written by 0x757360.
    pub const AT_0234: SharedField<u16> = SharedField::grouped(0x234, 26);

    /// Read by 0x75738c; written by 0x7573ac.
    pub const AT_0236: SharedField<u8> = SharedField::grouped(0x236, 26);

    /// Read by 0x75729c; written by 0x7572c0.
    pub const AT_0238: SharedField<i16> = SharedField::grouped(0x238, 26);

    /// Read by 0x7573d4; written by 0x7573f4.
    pub const AT_023C: SharedField<u32> = SharedField::grouped(0x23c, 26);

    /// Read by 0x75741c; written by 0x75743c.
    pub const AT_0240: SharedField<[u8; 4]> = SharedField::grouped(0x240, 26);
}

/// Access group 27. The window calculation's status and result, and the words around them.
pub mod group_27 {
    use super::*;

    /// Read by 0x757470; written by 0x757494.
    pub const AT_0244: SharedField<i16> = SharedField::grouped(0x244, 27);

    /// Read by 0x7574c0; written by 0x7574e4.
    pub const AT_0246: SharedField<i16> = SharedField::grouped(0x246, 27);

    /// Read by 0x757510; written by 0x757534.
    pub const AT_0248: SharedField<i16> = SharedField::grouped(0x248, 27);

    /// Read by 0x757560; written by 0x757584.
    pub const AT_024A: SharedField<i16> = SharedField::grouped(0x24a, 27);

    /// Read by 0x7575b0; written by 0x7575d4.
    pub const AT_024C: SharedField<i16> = SharedField::grouped(0x24c, 27);

    /// Read by 0x757600; written by 0x757624.
    pub const AT_024E: SharedField<i16> = SharedField::grouped(0x24e, 27);

    /// Read by 0x757650; written by 0x757674.
    pub const AT_0250: SharedField<i16> = SharedField::grouped(0x250, 27);

    /// Read by 0x7576a0; written by 0x7576c4.
    pub const AT_0252: SharedField<i16> = SharedField::grouped(0x252, 27);

    /// Read by 0x7576f0; written by 0x757714.
    pub const AT_0254: SharedField<i16> = SharedField::grouped(0x254, 27);

    /// Read by 0x7577e0; written by 0x757800.
    pub const AT_0258: SharedField<u32> = SharedField::grouped(0x258, 27);

    /// Read by 0x757828; written by 0x75784c.
    pub const AT_025C: SharedField<i16> = SharedField::grouped(0x25c, 27);

    /// Written by 0x757764.
    pub const AT_025E: SharedField<u16, Unread, SharedWrite> = SharedField::new(0x25e, Unread, SharedWrite::Granted(AccessGroup::at(27)));

    /// Written by 0x7577b4.
    pub const AT_0260: SharedField<u16, Unread, SharedWrite> = SharedField::new(0x260, Unread, SharedWrite::Granted(AccessGroup::at(27)));

    /// Read by 0x757878; written by 0x75789c.
    pub const AT_0262: SharedField<i16> = SharedField::grouped(0x262, 27);

    /// Read by 0x7578c8; written by 0x7578ec.
    pub const AT_0264: SharedField<i16> = SharedField::grouped(0x264, 27);

    /// Written by 0x75793c.
    pub const AT_0266: SharedField<u16, Unread, SharedWrite> = SharedField::new(0x266, Unread, SharedWrite::Granted(AccessGroup::at(27)));

    /// Read by 0x757968; written by 0x75798c.
    pub const AT_0268: SharedField<i16> = SharedField::grouped(0x268, 27);

    /// Read by 0x7579b8; written by 0x7579dc.
    pub const AT_026A: SharedField<i16> = SharedField::grouped(0x26a, 27);

    /// Read by 0x758a0c; written by 0x758a30.
    pub const AT_06F8: SharedField<i16> = SharedField::grouped(0x6f8, 27);

    /// Read by 0x758a5c; written by 0x758a80.
    pub const AT_06FA: SharedField<i16> = SharedField::grouped(0x6fa, 27);

    /// Read by 0x758aac; written by 0x758ad0.
    pub const AT_06FC: SharedField<i16> = SharedField::grouped(0x6fc, 27);

    /// Read by 0x758afc; written by 0x758b20.
    pub const AT_06FE: SharedField<i16> = SharedField::grouped(0x6fe, 27);

    /// Read by 0x758b4c; written by 0x758b70.
    pub const AT_0700: SharedField<i16> = SharedField::grouped(0x700, 27);

    /// Read by 0x758b9c; written by 0x758bc0.
    pub const AT_0702: SharedField<u16> = SharedField::grouped(0x702, 27);

    /// Read by 0x758bec; written by 0x758c0c.
    pub const AT_0704: SharedField<u32> = SharedField::grouped(0x704, 27);

    /// Read by 0x757b78; written by 0x757b98.
    pub const AT_0708: SharedField<u32> = SharedField::grouped(0x708, 27);

    /// Read by 0x757bc0; written by 0x757be4.
    pub const AT_070C: SharedField<i16> = SharedField::grouped(0x70c, 27);

    /// Read by 0x757c10; written by 0x757c34.
    pub const AT_070E: SharedField<i16> = SharedField::grouped(0x70e, 27);

    /// Read by 0x757c60; written by 0x757c84.
    pub const AT_0710: SharedField<i16> = SharedField::grouped(0x710, 27);

    /// Read by 0x757cb0; written by 0x757cd4.
    pub const AT_0712: SharedField<i16> = SharedField::grouped(0x712, 27);

    /// Read by 0x757d50; written by 0x757d74.
    pub const AT_0714: SharedField<i16> = SharedField::grouped(0x714, 27);

    /// Read by 0x757da0; written by 0x757dc4.
    pub const AT_0716: SharedField<i16> = SharedField::grouped(0x716, 27);

    /// Read by 0x757d00; written by 0x757d24.
    pub const AT_0718: SharedField<i16> = SharedField::grouped(0x718, 27);

    /// Read by 0x757df0; written by 0x757e10.
    pub const AT_071C: SharedField<u32> = SharedField::grouped(0x71c, 27);

    /// Read by 0x757e38; written by 0x757e58.
    pub const AT_0720: SharedField<u32> = SharedField::grouped(0x720, 27);

    /// Read by 0x757e80; written by 0x757ea0.
    pub const AT_0724: SharedField<u32> = SharedField::grouped(0x724, 27);

    /// Read by 0x757ec8; written by 0x757eec.
    pub const AT_0728: SharedField<u16> = SharedField::grouped(0x728, 27);

    /// Read by 0x757f18; written by 0x757f38.
    pub const AT_072A: SharedField<u8> = SharedField::grouped(0x72a, 27);

    /// Read by 0x757f60; written by 0x757f80.
    pub const AT_072C: SharedField<u32> = SharedField::grouped(0x72c, 27);

    /// Written by 0x757fc8.
    pub const AT_0730: SharedField<u32, Unread, SharedWrite> = SharedField::new(0x730, Unread, SharedWrite::Granted(AccessGroup::at(27)));

    /// The latest window calculation status (FocusRound `WindowStatus`).
    /// Read by 0x7580ec; written by 0x75810c.
    pub const WINDOW_STATUS: SharedField<u32> = SharedField::grouped(0x734, 27);

    /// Read by 0x758134; written by 0x758154.
    pub const AT_0738: SharedField<u32> = SharedField::grouped(0x738, 27);

    /// Read by 0x74e68c; written by 0x74e6ac.
    pub const AT_073C: SharedField<u32> = SharedField::grouped(0x73c, 27);

    /// Read by 0x74e7a0; written by 0x74e7c4.
    pub const AT_0740: SharedField<u16> = SharedField::grouped(0x740, 27);

    /// Read by 0x7581e8; written by 0x758208.
    pub const AT_0742: SharedField<u8> = SharedField::grouped(0x742, 27);

    /// Read by 0x758230; written by 0x758250.
    pub const AT_0744: SharedField<u32> = SharedField::grouped(0x744, 27);

    /// Read by 0x758278; written by 0x758298.
    pub const AT_0748: SharedField<u32> = SharedField::grouped(0x748, 27);

    /// Read by 0x7582c0; written by 0x758300.
    pub const AT_07EC: SharedField<[u8; 40]> = SharedField::grouped(0x7ec, 27);

    /// FocusRound `WindowPosition`, two halfwords.
    /// Read by 0x75817c; written by 0x7581b4.
    pub const WINDOW_POSITION: SharedField<u32> = SharedField::grouped(0x838, 27);
}

/// Access group 29.
pub mod group_29 {
    use super::*;

    /// Read by 0x7583b0; written by 0x7583d0.
    pub const AT_079C: SharedField<u32> = SharedField::grouped(0x79c, 29);
}

/// Access group 33.
pub mod group_33 {
    use super::*;

    /// Read by 0x7588e8; written by 0x75890c.
    pub const AT_07A0: SharedField<[u8; 76]> = SharedField::grouped(0x7a0, 33);
}

/// Access group 34.
pub mod group_34 {
    use super::*;

    /// Read by 0x758940; written by 0x758980.
    pub const AT_0814: SharedField<[u8; 36]> = SharedField::grouped(0x814, 34);
}

/// Access group 35.
pub mod group_35 {
    use super::*;

    /// Read by 0x7589c0; written by 0x7589e0.
    pub const AT_083C: SharedField<u32> = SharedField::grouped(0x83c, 35);
}

/// Access group 36.
pub mod group_36 {
    use super::*;

    /// Written by 0x758c34.
    pub const AT_0840: SharedField<[u8; 28], Unread, SharedWrite> = SharedField::new(0x840, Unread, SharedWrite::Granted(AccessGroup::at(36)));
}

/// Access group 37.
pub mod group_37 {
    use super::*;

    /// Read by 0x758c98; written by 0x758cc4.
    pub const AT_085C: SharedField<[u8; 8]> = SharedField::grouped(0x85c, 37);
}

/// Access group 38.
pub mod group_38 {
    use super::*;

    /// Read by 0x75a238; written by 0x75a210.
    pub const AT_0894: SharedField<u32> = SharedField::grouped(0x894, 38);
}

/// Access group 39.
pub mod group_39 {
    use super::*;

    /// Written by 0x75a258.
    pub const AT_0898: SharedField<[u8; 48], Unread, SharedWrite> = SharedField::new(0x898, Unread, SharedWrite::Granted(AccessGroup::at(39)));
}

/// Access group 41.
pub mod group_41 {
    use super::*;

    /// Read by 0x75a350; written by 0x75a324.
    pub const AT_096C: SharedField<i16> = SharedField::grouped(0x96c, 41);

    /// Read by 0x75a3a0; written by 0x75a374.
    pub const AT_096E: SharedField<i16> = SharedField::grouped(0x96e, 41);

    /// Read by 0x75a3f0; written by 0x75a3c4.
    pub const AT_0970: SharedField<i16> = SharedField::grouped(0x970, 41);

    /// Read by 0x75a440; written by 0x75a414.
    pub const AT_0972: SharedField<i16> = SharedField::grouped(0x972, 41);

    /// Read by 0x75a4ac; written by 0x75a464.
    pub const AT_0974: SharedArray<[u8; 8], 5, SharedRead, SharedWrite> = SharedArray::new(SharedField::new(0x974, SharedRead::Open, SharedWrite::Granted(AccessGroup::at(41))), 0x8);

    /// Read by 0x75a4ec; written by 0x75a52c.
    pub const AT_099C: SharedField<[u8; 44], SharedRead, SharedWrite> = SharedField::new(0x99c, SharedRead::Requires(AccessGroup::at(41)), SharedWrite::Open);
}

/// Access group 42.
pub mod group_42 {
    use super::*;

    /// Read by 0x75a58c; written by 0x75a564.
    pub const AT_09C8: SharedField<u32> = SharedField::grouped(0x9c8, 42);

    /// Read by 0x75a5d4; written by 0x75a5ac.
    pub const AT_09CC: SharedField<u8> = SharedField::grouped(0x9cc, 42);

    /// Read by 0x75a61c; written by 0x75a5f4.
    pub const AT_09D0: SharedField<u32> = SharedField::grouped(0x9d0, 42);
}
