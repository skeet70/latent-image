//! Filesystem results translated to the client status codes stock callbacks receive, and the
//! media readiness test the posters run before posting.

/// A raw filesystem result, 0 to 38; any other word translates as an unknown error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawStatus(pub u32);

/// The status word a stock completion callback receives, 0 to 14.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientStatus(pub u32);

impl ClientStatus {
    pub const OK: ClientStatus = ClientStatus(0);
    pub const FAILED: ClientStatus = ClientStatus(5);
}

/// A code the media report service at 0x726c74 receives in category 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaReport(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Translation {
    pub status: ClientStatus,
    pub report: Option<MediaReport>,
}

/// A status and a report code, zero for none.
type Row = (u8, u8);

const fn quiet(status: u8) -> Row {
    (status, 0)
}

const fn reported(status: u8, report: u8) -> Row {
    (status, report)
}

fn row(row: Row) -> Translation {
    Translation {
        status: ClientStatus(u32::from(row.0)),
        report: if row.1 == 0 { None } else { Some(MediaReport(u32::from(row.1))) },
    }
}

/// Indexed by raw status, from the jump table at 0x71d464.
const TRANSLATIONS: [Row; 39] = [
    quiet(0),
    reported(1, 0x50),
    quiet(5),
    reported(8, 0x61),
    reported(8, 0x62),
    quiet(5),
    quiet(5),
    quiet(6),
    quiet(6),
    reported(5, 0x57),
    reported(5, 0x57),
    quiet(5),
    quiet(5),
    reported(14, 0x64),
    reported(3, 0x60),
    quiet(5),
    quiet(5),
    reported(7, 0x66),
    reported(4, 0x56),
    quiet(8),
    reported(9, 0x67),
    quiet(10),
    quiet(11),
    reported(5, 0x58),
    quiet(5),
    quiet(5),
    reported(5, 0x58),
    quiet(5),
    quiet(5),
    quiet(5),
    quiet(5),
    quiet(5),
    reported(3, 0x5f),
    reported(13, 0x54),
    reported(12, 0x52),
    quiet(8),
    reported(8, 0x65),
    quiet(8),
    reported(5, 0x5b),
];

pub fn translate(raw: RawStatus) -> Translation {
    row(usize::try_from(raw.0).ok().and_then(|index| TRANSLATIONS.get(index)).copied().unwrap_or(quiet(5)))
}

/// The mode word a caller passes; the open, read, write and close posters pass 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadinessMode(pub u32);

/// Absent with no active context or a context whose device is -1; otherwise the context's
/// flag byte at +0x16bd8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Absent,
    Present { flags: u8 },
}

/// The first flag in bit order decides: bit 0 reports 0x50 and refuses with 1; bit 1
/// refuses with 5 and report 0x55 below mode 2; bit 2 refuses with 5 and report 0x53;
/// bit 3 refuses with 2 and report 0x52 in mode 0. Absent media refuses with 5.
pub fn readiness(media: Media, mode: ReadinessMode) -> Translation {
    let Media::Present { flags } = media else {
        return row(quiet(5));
    };
    row(if flags & 1 != 0 {
        reported(1, 0x50)
    } else if flags & 2 != 0 {
        if mode.0 >= 2 {
            quiet(0)
        } else {
            reported(5, 0x55)
        }
    } else if flags & 4 != 0 {
        reported(5, 0x53)
    } else if flags & 8 != 0 && mode.0 == 0 {
        reported(2, 0x52)
    } else {
        quiet(0)
    })
}
