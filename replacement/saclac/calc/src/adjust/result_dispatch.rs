//! The result kind a record dispatches with, its adoption into the round's statistics word, and
//! the record's dispatch option.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_13};
use xm1_saclac_profile_records::stock::shared::group_20;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::configuration::ProfileStore;
use crate::exposure::ports::ProfileAccess;

/// The state word image 4 keeps at 0x9134f8 (0x742054).
pub trait ServiceState {
    fn service_state(&mut self) -> u32;
}

/// The record's second dispatch result (0xfaaddc; its first, 0xfaaea8, is always 1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultKind {
    /// Selected for measurement; `kind_six` when record byte +0x34 is 6.
    Selected { kind_six: bool },
    /// Record byte +0x67 set.
    Flagged,
    /// Header byte 0x47 zero.
    Unclassed,
    Classed,
}

impl ResultKind {
    pub const fn code(self) -> u32 {
        match self {
            Self::Selected { kind_six: true } => 4,
            Self::Selected { kind_six: false } => 5,
            Self::Flagged => 12,
            Self::Unclassed => 10,
            Self::Classed => 3,
        }
    }
}

pub fn second_result(ports: &mut impl ProfileAccess, record: RecordHandle) -> ResultKind {
    if ports.profile().measurement_selection(record) == MeasurementSelection::Record {
        let kind: u8 = ports.profile().read(record, &group_00::AT_0034);
        return ResultKind::Selected { kind_six: kind == 6 };
    }
    let flagged: u8 = ports.profile().read(record, &group_00::AT_0067);
    if flagged != 0 {
        ResultKind::Flagged
    } else if ports.profile().header(record, SharedHeaderByte::At47) == 0 {
        ResultKind::Unclassed
    } else {
        ResultKind::Classed
    }
}

/// What became of the record's result against the round's statistics word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Adoption {
    Unchanged,
    /// The result replaced the word.
    Adopted,
    Refused,
}

/// Compares the record's second result with the round's statistics word (shared group 20 +0x14c):
/// equal leaves it, a change between codes 3 and 10 replaces it unless record word +0x20b0 is 3,
/// any other change is refused, and modes 3 and 4 refuse outright (0xfaae0c).
pub fn adopt_result(ports: &mut impl ProfileStore, record: RecordHandle, mode: u32) -> Adoption {
    if (3..=4).contains(&mode) {
        return Adoption::Refused;
    }
    let result = second_result(ports, record).code();
    let current: u32 = ports.profile().read_shared(record, &group_20::AT_014C);
    if result == current {
        return Adoption::Unchanged;
    }
    if !matches!((result, current), (10, 3) | (3, 10)) {
        return Adoption::Refused;
    }
    let locked: u32 = ports.profile().read(record, &group_13::AT_20B0_4);
    if locked == 3 {
        return Adoption::Refused;
    }
    let _refused = ports.records().write_shared(record, &group_20::AT_014C, result);
    Adoption::Adopted
}

/// The record's dispatch option, record byte +0x6e; 0 for header byte 0x47 of 0, 4, 5 or 6, for
/// record byte +0x70 set, for a nonzero chosen word, or while the service state is 1 (0xfaac8c).
pub fn dispatch_option(ports: &mut impl ProfileStore, service: &mut impl ServiceState, record: RecordHandle) -> u8 {
    if matches!(ports.profile().header(record, SharedHeaderByte::At47), 0 | 4 | 5 | 6) {
        return 0;
    }
    let held: u8 = ports.profile().read(record, &group_00::AT_0070);
    if held != 0 || ports.records().chosen_word(record).unwrap_or(0) != 0 || service.service_state() == 1 {
        return 0;
    }
    ports.profile().read(record, &group_00::AT_006E)
}
