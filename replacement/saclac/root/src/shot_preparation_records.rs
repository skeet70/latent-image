#![forbid(unsafe_code)]

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{FocusConfirmation, FocusHistory, FocusResult, LensSnapshot, Publication, PublishedList, Records, Setting, SummaryWord}, stock as shot, summary::{AreaStatistics, ENTRIES}};
use crate::{shot_services::Libraries, stock::acquisition::read_record};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Records for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn setting(&mut self, setting: Setting) -> u32 {
        let mut services = self.services.borrow_mut();
        match setting {
            Setting::ContextMode => u32::from(xm1_saclac_calc::profile::selection_code(services.entries.measurement_selection(self.record))),
            Setting::LensPolicy => u32::from(services.entries.header_byte(self.record, SharedHeaderByte::At47)),
            Setting::PrimarySubjects => u32::from(services.entries.header_byte(self.record, SharedHeaderByte::At61)),
            Setting::SecondarySubjects => u32::from(services.field(self.record, &r::group_00::AT_005D)),
            Setting::SelectionStyle => services.field(self.record, &r::group_13::AT_20B0_4),
            Setting::SelectionConfirmed => u32::from(services.field(self.record, &r::group_00::AT_0066)),
            Setting::SelectionKind => u32::from(services.field(self.record, &r::group_00::AT_004F)),
            Setting::SelectionOption => u32::from(services.entries.header_byte(self.record, SharedHeaderByte::At31)),
            Setting::CaptureVariant => u32::from(services.field(self.record, &r::group_00::AT_0064)),
            Setting::AreaLayout => services.field(self.record, &r::group_00::AT_0000),
            Setting::SubjectQueryA => services.field(self.record, &r::group_04::AT_0164),
            Setting::SubjectQueryB => services.field(self.record, &r::group_04::AT_0168),
            Setting::DumpArgument => u32::from(services.field(self.record, &r::group_00::INTERVAL_TRACKING).0),
            Setting::ExposureSource => u32::from(services.field(self.record, &r::group_00::AT_0044)),
            Setting::StatisticsOption => u32::from(services.field(self.record, &r::group_00::AT_0036)),
            Setting::CorrectionExport => u32::from(services.field(self.record, &r::group_00::AT_0043)),
        }
    }
    fn lens_snapshot(&mut self) -> LensSnapshot { shot::lens_snapshot(&self.services.borrow().shared_bytes(self.record, &s::group_17::LENS_SNAPSHOT)) }
    fn store_lens_snapshot(&mut self, value: &LensSnapshot) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_17::LENS_SNAPSHOT, shot::lens_snapshot_bytes(value));
    }
    fn focus_result(&mut self) -> FocusResult { shot::focus_result(&self.services.borrow().shared_bytes(self.record, &s::group_14::FOCUS_RESULT)) }
    fn store_focus_result(&mut self, value: &FocusResult) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_14::FOCUS_RESULT, shot::focus_result_bytes(value));
    }
    fn focus_confirmation(&mut self) -> FocusConfirmation { shot::focus_confirmation(&self.services.borrow().shared_bytes(self.record, &s::group_15::FOCUS_CONFIRMATION)) }
    fn store_focus_confirmation(&mut self, value: &FocusConfirmation) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_15::FOCUS_CONFIRMATION, shot::focus_confirmation_bytes(value));
    }
    fn focus_history(&mut self) -> FocusHistory { shot::focus_history(&self.services.borrow().shared_bytes(self.record, &s::group_25::FOCUS_HISTORY)) }
    fn store_focus_history(&mut self, value: &FocusHistory) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_25::FOCUS_HISTORY, shot::focus_history_bytes(value));
    }
    fn clear_exposure_record(&mut self) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_18::AT_00E0, [0; 64]);
    }
    fn area_statistics(&mut self) -> AreaStatistics {
        let services = self.services.borrow();
        let descriptor = services.shared_bytes(self.record, &s::group_16::AREA_STATISTICS);
        if u32::from_le_bytes(descriptor[..4].try_into().expect("statistics presence")) == 0 {
            return AreaStatistics { present: 0, layout: descriptor[4], ..AreaStatistics::default() };
        }
        let address = u32::from_le_bytes(descriptor[8..12].try_into().expect("statistics entries"));
        let entries = read_record::<{ ENTRIES * shot::AREA_ENTRY_BYTES }>(&services.machine, SharedAddress(address));
        shot::area_statistics(&descriptor, &entries)
    }
    fn publish(&mut self, publication: Publication<'_>) {
        let mut services = self.services.borrow_mut();
        macro_rules! store { ($field:expr, $value:expr) => {{ let _ = services.records.write(self.record, $field, $value); }}; }
        macro_rules! element { ($array:expr, $index:expr, $value:expr) => {{ if let Some(index) = Index::new($index) { let _ = services.records.write_element(self.record, $array, index, $value); } }}; }
        match publication {
            Publication::Subject { list, index, subject } => match list {
                PublishedList::Selected => element!(&r::group_30::AT_07A4, index, shot::subject_bytes(subject)),
                PublishedList::Tracked => element!(&r::group_30::AT_1880, index, shot::subject_bytes(subject)),
            },
            Publication::SubjectCount { list, count } => match list {
                PublishedList::Selected => store!(&r::group_30::AT_07A0, count),
                PublishedList::Tracked => store!(&r::group_30::AT_187C, count),
            },
            Publication::SubjectFlag { index, value } => element!(&r::group_30::AT_1864, index, value as i16),
            Publication::SubjectSummary { word, value } => match word {
                SummaryWord::First => store!(&r::group_30::AT_17F0, value as i16),
                SummaryWord::Second => store!(&r::group_30::AT_17F2, value as i16),
                SummaryWord::Third => store!(&r::group_30::AT_17F4, value as i16),
                SummaryWord::Count => store!(&r::group_30::AT_17F8, value),
            },
            Publication::SelectedSubject(subject) => store!(&r::group_30::AT_17FC, shot::subject_bytes(subject)),
            Publication::SubjectTime(value) => store!(&r::group_30::AT_20A8, value),
            Publication::TrackedMarker(value) => store!(&r::group_30::AT_20A4, value),
            Publication::AreaSelection(pointer) => {
                let machine = services.machine.clone();
                let _ = services.records.write_with(self.record, &r::group_01::AT_0090_56, || read_record::<56>(&machine, SharedAddress(pointer)));
            }
            Publication::Area { index, word: pointer } => {
                if let Some(index) = Index::new(index) {
                    let machine = services.machine.clone();
                    let _ = services.records.write_with(self.record, &r::group_05::AT_016C_168.at(index), || read_record::<168>(&machine, SharedAddress(pointer)));
                }
            }
            Publication::LensTargetArea(pointer) => {
                let machine = services.machine.clone();
                let _ = services.records.write_with(self.record, &r::group_02::AT_00C8_124, || read_record::<124>(&machine, SharedAddress(pointer.0)));
            }
            Publication::CaptureFrame(frame) => { let _ = services.records.write_shared(self.record, &s::group_35::AT_083C, frame.map_or(0, |frame| frame.0)); }
            Publication::CaptureLink(value) => { let _ = services.records.write_shared(self.record, &s::group_14::AT_0890, value); }
        }
    }
    fn select_first_mode(&mut self) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(0x742a6c, &[1, 0]);
    }
    fn load_lens_capabilities(&mut self) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(0x740690, &[]);
    }
}
