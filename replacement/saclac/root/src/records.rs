#![forbid(unsafe_code)]

//! The measurement round's view of the shared measurement record and the profile records, and
//! the shared layer's profile headers, over the owned profile-record view.

use xm1_focus_measurement_round::lanes::Selection;
use xm1_focus_measurement_round::profile::{
    FrameInterval, GridCell, IntervalHistory, IntervalTracking, Profile, ProfileGeometry, ProfileMode, ProfileRecords,
    ProfileSelection, PublishedResult, RetainedHalfword, RetainedWord, StatisticsInUse, WindowPosition, WindowRecord,
    WindowStatus,
};
use xm1_focus_measurement_round::stock::{availability, RecordSelection};
use xm1_saclac_interface::round::{GeometryPair, RetainedGeometryValue};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::field::{SharedField, SharedRead, SharedWrite, Value};
use xm1_saclac_profile_records::stock::record::{group_00, group_04, group_13};
use xm1_saclac_profile_records::stock::shared::{group_14, group_15, group_20, group_22, group_23, group_25, group_27};
use xm1_saclac_profile_records::{
    AccessGroup, ChangeSet, MeasurementSelection, Memory, SharedHeaderByte, StockEntries,
};
use xm1_saclac_shared::settings::{self, ProfileHeaders};
use xm1_saclac_shared::stock::Machine;

use crate::services::{Services, NO_HANDLE};
use crate::stock;

/// The group whose write grant covers the measurement history's position (0x756814).
const HISTORY_GROUP: AccessGroup = match AccessGroup::new(16) {
    Some(group) => group,
    None => panic!("group 16 is a profile-record access group"),
};

/// 0xf32618 tests bit 7 of the first change mask, the measurement resource's (0x74ec30).
const GEOMETRY_CHANGED: ChangeSet = match group_00::AT_0000.change() {
    Some(bit) => ChangeSet::of(bit),
    None => panic!("the measurement resource is change-tracked"),
};

/// Words of the interval history (0x757ff0) before its count halfword at +40.
const INTERVAL_WORDS: usize = IntervalHistory::CAPACITY;

const fn selection(selection: MeasurementSelection) -> ProfileSelection {
    match selection {
        MeasurementSelection::Unselected => ProfileSelection::Unselected,
        MeasurementSelection::Record => ProfileSelection::Record,
        MeasurementSelection::Other(byte) => ProfileSelection::Other(byte),
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries> Services<M, R, E> {
    /// Zeroes `bytes` of a shared field, read ungated and written under `record`'s grant, as
    /// 0xf32a48 marks a published result invalid.
    fn clear_shared<const N: usize>(&mut self, record: RecordHandle, field: &SharedField<[u8; N], SharedRead, SharedWrite>, bytes: core::ops::Range<usize>) {
        let mut value = self.shared_bytes(NO_HANDLE, field);
        value[bytes].fill(0);
        let _refused = self.records.write_shared(record, field, value);
    }

    fn store<T: Value>(&mut self, record: RecordHandle, field: &SharedField<T, SharedRead, SharedWrite>, value: T) {
        let _refused = self.records.write_shared(record, field, value);
    }

    /// 0xf5206c: header byte 0x47 chooses the eight bytes at +4, the halfword at +0xc or cell 3, 3.
    fn focus_cell(&self, record: RecordHandle, header: u8) -> GridCell {
        match header {
            2 | 5 => {
                let bytes = self.field(record, &group_00::AT_0004);
                let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
                GridCell { column: word(0), row: word(4) }
            }
            0 => {
                let packed = self.field(record, &group_00::AT_000C);
                GridCell { column: u32::from(packed & 0xff), row: u32::from(packed >> 8) }
            }
            _ => GridCell { column: 3, row: 3 },
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries> ProfileRecords for Services<M, R, E> {
    fn offered(&mut self) -> Option<Selection> {
        availability(RecordSelection(self.shared(NO_HANDLE, &group_20::OFFERED_MEASUREMENTS).0))
    }

    fn statistics_in_use(&mut self) -> StatisticsInUse {
        let in_use = self.shared(NO_HANDLE, &group_20::STATISTICS_IN_USE);
        StatisticsInUse { primary: in_use.primary, search: in_use.search, auxiliary: in_use.auxiliary }
    }

    fn set_statistics_in_use(&mut self, record: RecordHandle, in_use: StatisticsInUse) {
        let in_use = xm1_saclac_profile_records::StatisticsInUse {
            primary: in_use.primary,
            search: in_use.search,
            auxiliary: in_use.auxiliary,
        };
        self.store(record, &group_20::STATISTICS_IN_USE, in_use);
    }

    fn detach_capture_buffer(&mut self, record: RecordHandle) { self.store(record, &group_20::CAPTURE_BUFFER, None) }

    /// 0xf32a48: word 1 of the focus result, word 0 of the confirmation, word 3 of +0x180 and
    /// word 0 of +0x194.
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult) {
        match result {
            PublishedResult::Acquisition => self.clear_shared(record, &group_14::FOCUS_RESULT, 4..8),
            PublishedResult::Search => self.clear_shared(record, &group_15::FOCUS_CONFIRMATION, 0..4),
            PublishedResult::Auxiliary => self.clear_shared(record, &group_22::AT_0180, 12..16),
            PublishedResult::Source => self.clear_shared(record, &group_23::AT_0194, 0..4),
        }
    }

    fn invalidate_focus_result(&mut self) {
        let controller = record_handle_from_a2(self.machine.read_u32(stock::FOCUS_CONTROLLER_RECORD));
        self.clear_shared(controller, &group_25::FOCUS_HISTORY, 0..4);
    }

    fn grants_measurement_history(&mut self, record: RecordHandle) -> bool { self.records.may_write(record, HISTORY_GROUP) }

    fn acquisition_level(&mut self) -> i16 { self.shared(NO_HANDLE, &group_27::AT_0250) }

    fn interval_history(&mut self) -> IntervalHistory {
        let bytes = self.shared_bytes(NO_HANDLE, &group_14::AT_0864);
        let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let intervals = core::array::from_fn(|index| FrameInterval(word(4 * index)));
        let count = u16::from_le_bytes([bytes[4 * INTERVAL_WORDS], bytes[4 * INTERVAL_WORDS + 1]]);
        IntervalHistory::new(intervals, usize::from(count))
    }

    /// Bytes 42..48 keep their value; intervals past the count are zero, as 0xf32c74 leaves them.
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory) {
        let mut bytes = self.shared_bytes(NO_HANDLE, &group_14::AT_0864);
        bytes[..4 * INTERVAL_WORDS].fill(0);
        for (slot, interval) in bytes.chunks_exact_mut(4).zip(history.intervals()) {
            slot.copy_from_slice(&interval.0.to_le_bytes());
        }
        let count = history.intervals().len() as u16;
        bytes[4 * INTERVAL_WORDS..4 * INTERVAL_WORDS + 2].copy_from_slice(&count.to_le_bytes());
        self.store(record, &group_14::AT_0864, bytes);
    }

    fn profile(&mut self, record: RecordHandle) -> Profile {
        let header = self.entries.header_byte(record, SharedHeaderByte::At47);
        Profile {
            selection: selection(self.entries.measurement_selection(record)),
            mode: ProfileMode(self.field(record, &group_00::AT_003E)),
            interval_tracking: match self.field(record, &group_00::INTERVAL_TRACKING).0 {
                0 => IntervalTracking::Clear,
                1 => IntervalTracking::Record,
                _ => IntervalTracking::Keep,
            },
            geometry_on_autofocus: self.records.has_changes(record, GEOMETRY_CHANGED).unwrap_or(false),
            profile_cadence: self.field(record, &group_13::AT_20CC) == 1,
            frame_rate_cadence: self.field(record, &group_13::AT_20D4) == 1,
            focus_cell: self.focus_cell(record, header),
            takes_search_adjustment: header == 6,
        }
    }

    /// 0xf3292c.
    fn geometry(&mut self, record: RecordHandle) -> ProfileGeometry {
        let resource = self.field(record, &group_00::AT_0000);
        ProfileGeometry {
            resource: GeometryPair([resource as u16, (resource >> 16) as u16]),
            converted_profile: RetainedGeometryValue(self.records.chosen_word(record).unwrap_or(0)),
            first_parameter: RetainedGeometryValue(self.field(record, &group_04::AT_0164)),
            second_parameter: RetainedGeometryValue(self.field(record, &group_04::AT_0168)),
        }
    }

    /// The window request's record (0xf3a564).
    fn window(&mut self) -> WindowRecord {
        WindowRecord {
            status: WindowStatus(self.shared(NO_HANDLE, &group_27::WINDOW_STATUS).0),
            first: RetainedWord(self.shared(NO_HANDLE, &group_20::AT_014C)),
            halfword: RetainedHalfword(self.shared(NO_HANDLE, &group_27::AT_0710) as u16),
            second: RetainedWord(self.shared(NO_HANDLE, &group_27::AT_0738)),
        }
    }

    fn store_window_status(&mut self, record: RecordHandle, status: WindowStatus) {
        self.store(record, &group_27::WINDOW_STATUS, xm1_saclac_profile_records::WindowStatus(status.0));
    }

    fn store_window_position(&mut self, record: RecordHandle, position: WindowPosition) {
        self.store(record, &group_27::WINDOW_POSITION, xm1_saclac_profile_records::WindowPosition(position.0));
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries> ProfileHeaders for Services<M, R, E> {
    fn measurement_selection(&mut self, record: RecordHandle) -> settings::MeasurementSelection {
        settings::MeasurementSelection(match self.entries.measurement_selection(record) {
            MeasurementSelection::Unselected => 0,
            MeasurementSelection::Record => 1,
            MeasurementSelection::Other(byte) => byte,
        })
    }

    fn mode(&mut self, record: RecordHandle) -> settings::ProfileMode {
        settings::ProfileMode(self.entries.header_byte(record, SharedHeaderByte::At47))
    }
}
