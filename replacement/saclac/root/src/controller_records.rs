#![forbid(unsafe_code)]

use xm1_saclac_controllers::layers::{Block, Change, Field, LensSlot, ProfileRecord, Restarting};
use xm1_saclac_interface::{stock::StockCallbackWord, RecordHandle};
use xm1_saclac_profile_records::field::Index;
use xm1_saclac_profile_records::stock::{record as r, shared as s};
use xm1_saclac_profile_records::{AccessGroup, ChangeSet, Memory, OfferedMeasurements, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use crate::services::Services;

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("catalog word"))
}

fn unpack(bytes: &[u8], out: &mut [u32]) {
    assert_eq!(out.len(), bytes.len().div_ceil(4));
    for (value, bytes) in out.iter_mut().zip(bytes.chunks(4)) {
        let mut packed = [0; 4];
        packed[..bytes.len()].copy_from_slice(bytes);
        *value = u32::from_le_bytes(packed);
    }
}

fn pack<const N: usize>(words: &[u32]) -> [u8; N] {
    assert_eq!(words.len(), N.div_ceil(4));
    let mut out = [0; N];
    for (bytes, word) in out.chunks_mut(4).zip(words) {
        bytes.copy_from_slice(&word.to_le_bytes()[..bytes.len()]);
    }
    out
}

fn lens_index(slot: LensSlot) -> Index<5> {
    Index::new(match slot { LensSlot::Reported => 0, LensSlot::Current => 2, LensSlot::Next => 3 }).expect("lens slot")
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ProfileRecord for Services<M, R, E, P> {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32 {
        macro_rules! record { ($group:ident :: $field:ident) => { self.field(record, &r::$group::$field) as u32 }; }
        macro_rules! shared { ($group:ident :: $field:ident) => { self.shared(record, &s::$group::$field) as u32 }; }
        match field {
            Field::ViewActive => record!(group_00::AT_005D),
            Field::SceneKind => u32::from(self.entries.header_byte(record, SharedHeaderByte::At31)),
            Field::Selection => match self.entries.measurement_selection(record) {
                xm1_saclac_profile_records::MeasurementSelection::Unselected => 0,
                xm1_saclac_profile_records::MeasurementSelection::Record => 1,
                xm1_saclac_profile_records::MeasurementSelection::Other(value) => u32::from(value),
            },
            Field::SelectionNotifies => record!(group_00::AT_0032),
            Field::PreparationFirst => shared!(group_20::AT_0140),
            Field::PreparationSecond => shared!(group_20::AT_0144),
            Field::PreparationThird => shared!(group_20::AT_0148),
            Field::PendingOutput => shared!(group_21::AT_0178),
            Field::ReleaseRange => word(&self.shared_bytes(record, &s::group_17::LENS_SNAPSHOT), 8),
            Field::ReleaseNotifies => shared!(group_14::AT_0890),
            Field::SelectionCallbackArmed => u32::from(self.entries.header_byte(record, SharedHeaderByte::At63)),
            Field::SelectionPending | Field::SelectionTaken => record!(group_31::AT_2110),
            Field::SelectionEnables => record!(group_13::AT_20F4),
            Field::SelectionFlags => record!(group_13::AT_20F8),
            Field::ViewMode | Field::Command1dFirst => record!(group_13::AT_20B0_4),
            Field::ViewLocked => record!(group_00::AT_0066),
            Field::PreparationRestorable => record!(group_00::AT_0067),
            Field::Command1dRestarts => u32::from(self.entries.header_byte(record, SharedHeaderByte::At47)),
            Field::AdjustmentTarget => shared!(group_20::AT_014C),
            Field::RefreshValue | Field::Command17Sixth => shared!(group_20::AT_0170),
            Field::AdjustmentFirst => record!(group_00::AT_004F),
            Field::AdjustmentSecond => record!(group_00::AT_0050),
            Field::Command17Selection => self.shared(record, &s::group_20::OFFERED_MEASUREMENTS).0,
            Field::Command17First => shared!(group_20::AT_0158),
            Field::Command17Fourth => self.records.read_shared(record, &s::group_20::IMAGE_CALLBACK).unwrap_or(StockCallbackWord(0)).0,
            Field::Command17Fifth => shared!(group_20::AT_016C),
            Field::PrepareFirst | Field::SelectionThird => record!(group_13::AT_20FC),
            Field::Command1dSecond | Field::OutputThird => record!(group_13::AT_20B4),
            Field::Command1eValue | Field::OutputFirst => record!(group_13::AT_20C8),
            Field::Command20Value | Field::CaptureFirst => record!(group_13::AT_20CC),
            Field::Command21Value | Field::FormatValue => record!(group_13::AT_20D4),
            Field::Command23Second | Field::ResultsCallback => record!(group_31::AT_2114),
            Field::StatisticsRetune => record!(group_00::AT_004D),
            Field::AutofocusClass => record!(group_00::AT_003E),
            Field::AutofocusApplied => shared!(group_27::AT_0744),
            Field::AutofocusRelease => word(&self.shared_bytes(record, &s::group_14::FOCUS_RESULT), 26),
            Field::LensRange => record!(group_00::AT_0044),
            Field::Descriptor => self.records.chosen_word(record).unwrap_or_default(),
            Field::DescriptorFirst => record!(group_04::AT_0164),
            Field::DescriptorSecond => record!(group_04::AT_0168),
            Field::FormatRatio => record!(group_00::AT_0000),
            Field::FormatMode => record!(group_00::AT_0034),
            Field::OutputFourth => record!(group_13::AT_20D0),
            Field::ExposureFirst => record!(group_00::AT_0035),
            Field::ExposureSecond => record!(group_00::AT_0036),
            Field::ExposureThird => record!(group_00::AT_0038),
            Field::ExposureSelected => record!(group_00::AT_003D),
            Field::ExposureSixth => record!(group_00::AT_0039),
            Field::ExposureSeventh => record!(group_00::AT_003A),
            Field::ExposureEighth => record!(group_00::AT_003B),
            Field::FactorFirst => record!(group_00::AT_0049),
            Field::FactorSecond => record!(group_00::AT_004A),
            Field::WindowLevel => shared!(group_27::AT_070C),
            Field::LensBase => shared!(group_27::AT_070E),
            Field::LensIndex => shared!(group_27::AT_071C),
            Field::LinearSource => shared!(group_27::AT_0716),
            Field::Detail => record!(group_00::AT_0059),
            Field::CaptureSecond => record!(group_00::AT_004C),
            Field::CaptureOptionFirst => record!(group_00::AT_005B),
            Field::IntervalTracking => u32::from(self.field(record, &r::group_00::INTERVAL_TRACKING).0),
            Field::CaptureOptionFourth => record!(group_00::AT_006D),
            Field::CaptureOptionFifth => record!(group_00::AT_0058),
            Field::CapturePolicy => shared!(group_27::AT_0740),
            Field::StartHeader => u32::from(self.entries.header_byte(record, SharedHeaderByte::At61)),
            Field::CaptureView => record!(group_00::AT_2128),
            Field::StartMode => shared!(group_27::AT_0738),
            Field::StopMode => self.shared(record, &s::group_27::WINDOW_STATUS).0,
            Field::DiagnosticBuffer => self.shared(record, &s::group_20::CAPTURE_BUFFER).map_or(0, |buffer| buffer.address()),
            Field::PipelineDumpBuffer => shared!(group_35::AT_083C),
            Field::CaptureWindowMode => self.records.read_element(record, &r::group_10::AT_020C, Index::new(0).expect("first capture mode")).unwrap_or_default(),
            Field::SpanMode => record!(group_02::AT_00C8_4),
            Field::SpanSet => record!(group_02::AT_00D0),
            Field::Level => shared!(group_26::AT_0206),
            Field::Adjusted => shared!(group_26::AT_0208),
            Field::LinearLevel => shared!(group_26::AT_0238),
            Field::LensWord => shared!(group_26::AT_0200),
            Field::Step => shared!(group_26::AT_0230),
            Field::StepLimit => shared!(group_26::AT_0234),
            Field::StepMode => shared!(group_26::AT_0236),
            Field::ExposureWord => shared!(group_26::AT_0218),
            Field::ExposureOffset => shared!(group_26::AT_0216),
            Field::ViewByte => record!(group_00::AT_0070),
            Field::WindowFirst => record!(group_00::AT_0048),
            Field::WindowSecond => record!(group_00::AT_002A),
            Field::WindowThird => u32::from(self.entries.header_byte(record, SharedHeaderByte::At46)),
            Field::WindowFourth => record!(group_00::AT_003F),
            Field::WindowMode => record!(group_01::AT_0090_2),
            Field::ResultsTaken => record!(group_30::AT_20A4),
            Field::ResultsCount => record!(group_30::AT_1C90),
            Field::ResultsTime => record!(group_30::AT_20AC),
            Field::SelectionTracked => u32::from(self.shared_bytes(record, &s::group_41::AT_099C)[41]),
            Field::LensDesignation => word(&self.entries.lens_designation(record), 0),
            Field::LensConversion => record!(group_00::AT_0064),
            Field::LensReference => shared!(group_27::AT_0720),
            Field::DisplayMode => record!(group_00::AT_0056),
            Field::DisplayModeFirst => word(&self.field(record, &r::group_00::AT_0072), 0),
            Field::DisplayModeSecond => word(&self.field(record, &r::group_00::AT_0076), 0),
            Field::DisplayModeByte => record!(group_00::AT_0071),
            Field::DisplayScanIndex => record!(group_00::AT_0057),
            Field::DisplayStatusByte => record!(group_00::AT_0068),
            Field::DisplayOutput => record!(group_00::AT_0052),
            Field::StopQueried | Field::Command17Second | Field::Command17Third | Field::PrepareSecond => panic!("controller read of a write-only field"),
        }
    }

    fn write(&mut self, record: RecordHandle, field: Field, value: u32) {
        macro_rules! record { ($group:ident :: $field:ident) => {{ let _ = self.records.write(record, &r::$group::$field, value as _); }}; }
        macro_rules! shared { ($group:ident :: $field:ident) => {{ let _ = self.records.write_shared(record, &s::$group::$field, value as _); }}; }
        match field {
            Field::PreparationFirst => shared!(group_20::AT_0140),
            Field::PreparationSecond => shared!(group_20::AT_0144),
            Field::PreparationThird => shared!(group_20::AT_0148),
            Field::PendingOutput => shared!(group_21::AT_0178),
            Field::SelectionEnables => record!(group_13::AT_20F4),
            Field::SelectionFlags => record!(group_13::AT_20F8),
            Field::SelectionTaken => record!(group_31::AT_2110),
            Field::AdjustmentTarget => shared!(group_20::AT_014C),
            Field::StopQueried => shared!(group_20::AT_0174),
            Field::Command17Selection => { let _ = self.records.write_shared(record, &s::group_20::OFFERED_MEASUREMENTS, OfferedMeasurements(value)); }
            Field::Command17First => shared!(group_20::AT_0158),
            Field::Command17Second => shared!(group_20::AT_015C),
            Field::Command17Third => shared!(group_20::AT_0160),
            Field::Command17Fourth => { let _ = self.records.write_shared(record, &s::group_20::IMAGE_CALLBACK, StockCallbackWord(value)); }
            Field::Command17Fifth => shared!(group_20::AT_016C),
            Field::Command17Sixth => shared!(group_20::AT_0170),
            Field::PrepareFirst => record!(group_13::AT_20FC),
            Field::PrepareSecond => {
                // 0x75564c -> 0x75d948: the scalar is the source pointer for four words.
                if self.records.may_write(record, AccessGroup::new(13).expect("profile group")) {
                    let bytes = pack::<16>(&core::array::from_fn::<_, 4, _>(|index| self.machine.read_u32(value.wrapping_add(4 * index as u32))));
                    let _ = self.records.write(record, &r::group_13::AT_2100, bytes);
                }
            }
            Field::Command1dFirst => record!(group_13::AT_20B0_4),
            Field::Command1dSecond => record!(group_13::AT_20B4),
            Field::Command1eValue => record!(group_13::AT_20C8),
            Field::Command20Value => record!(group_13::AT_20CC),
            Field::Command21Value => record!(group_13::AT_20D4),
            Field::Command23Second => record!(group_31::AT_2114),
            Field::AutofocusApplied => shared!(group_27::AT_0744),
            Field::AutofocusRelease => {
                let mut bytes = self.shared_bytes(record, &s::group_14::FOCUS_RESULT);
                bytes[0x68..0x6c].copy_from_slice(&value.to_le_bytes());
                let _ = self.records.write_shared(record, &s::group_14::FOCUS_RESULT, bytes);
            }
            Field::StartMode => shared!(group_27::AT_0738),
            Field::ResultsTaken => record!(group_30::AT_20A4),
            Field::ResultsCount => record!(group_30::AT_1C90),
            Field::ResultsTime => record!(group_30::AT_20AC),
            _ => panic!("controller write of a read-only scalar field"),
        }
    }

    fn changed(&mut self, record: RecordHandle, change: Change) -> bool {
        macro_rules! bit { ($field:ident) => { r::group_00::$field.change().expect("tracked header field") }; }
        let bit = match change {
            Change::ViewGeometry => bit!(AT_0047), Change::AutofocusArea => bit!(AT_005D),
            Change::AutofocusMode => bit!(AT_004F), Change::ModeDecides => bit!(MEASUREMENT_SELECTION),
            Change::SelectionMode => bit!(AT_0031), Change::SelectionOwner => bit!(AT_0063),
            Change::SelectionNotice => bit!(AT_003C), Change::SelectionReset => bit!(INTERVAL_TRACKING),
            Change::StopTwo => bit!(AT_0061), Change::StopFour => bit!(AT_005B), Change::StopFive => bit!(AT_0050),
            Change::Resource => bit!(AT_0034), Change::Shift => bit!(AT_0032), Change::Offset => bit!(AT_0039),
            Change::Scale => bit!(AT_003A), Change::Level => bit!(AT_003B), Change::Exposure => bit!(AT_0035),
            Change::Forced => bit!(AT_003E), Change::Class => bit!(AT_0000), Change::Geometry => bit!(AT_005A),
            Change::PropertyGated(index) => match index {
                0 => bit!(AT_0056), 1 => bit!(AT_0057), 2 => bit!(AT_0072), 3 => bit!(AT_0076), 4 => bit!(AT_0071),
                _ => panic!("property change index out of range"),
            },
            Change::Restarting(which) => match which {
                Restarting::First => bit!(AT_0067), Restarting::Second => bit!(AT_0058), Restarting::Third => bit!(AT_0068),
                Restarting::Fourth => bit!(AT_0070), Restarting::Fifth => bit!(AT_0069), Restarting::Sixth => bit!(AT_002C),
            },
            Change::Range => bit!(AT_004C), Change::Output => bit!(AT_0052), Change::Lens => bit!(AT_0041),
            Change::LensPosition => bit!(AT_0022), Change::LensRange => bit!(AT_0044), Change::LensWindow => bit!(AT_001E),
            Change::LensValue => bit!(AT_001C), Change::ExposureSecond => bit!(AT_0036), Change::ExposureThird => bit!(AT_0038),
            Change::FactorFirst => bit!(AT_0049), Change::FactorSecond => bit!(AT_004A), Change::Retune => bit!(AT_004D),
            Change::CaptureRatio => bit!(AT_0004), Change::CaptureOptionFourth => bit!(AT_006D), Change::Detail => bit!(AT_0059),
        };
        self.records.has_changes(record, ChangeSet::of(bit)).unwrap_or(false)
    }

    fn read_block(&mut self, record: RecordHandle, block: Block, out: &mut [u32]) {
        macro_rules! record { ($group:ident :: $field:ident) => { unpack(&self.field(record, &r::$group::$field), out) }; }
        macro_rules! shared { ($group:ident :: $field:ident) => { unpack(&self.shared_bytes(record, &s::$group::$field), out) }; }
        match block {
            Block::Command1f | Block::AuxiliaryWindow => record!(group_13::AT_20B8),
            Block::Prepare26 => shared!(group_37::AT_085C), Block::Suspension => shared!(group_33::AT_07A0),
            Block::Lens(slot) => unpack(&self.records.read_shared_element(record, &s::group_41::AT_0974, lens_index(slot)).unwrap_or([0; 8]), out),
            Block::SelectionRange => record!(group_13::AT_2100),
            Block::Spans => unpack(&self.records.read(record, &r::group_30::AT_17FC).unwrap_or([0; 104]), out),
            Block::Classes => record!(group_00::AT_0004), Block::WindowBytes => record!(group_00::AT_000E),
            Block::ReleaseRange => shared!(group_17::LENS_SNAPSHOT), Block::Options => shared!(group_19::AT_0120),
            Block::LensTarget => shared!(group_26::AT_0240), Block::Release => shared!(group_14::FOCUS_RESULT),
            Block::FocusConfirmation => shared!(group_15::FOCUS_CONFIRMATION), Block::FocusHistory => shared!(group_25::FOCUS_HISTORY),
            Block::MeasuredWindow => shared!(group_22::AT_0180),
            Block::ResultRecord(index) => unpack(&self.records.read_element(record, &r::group_30::AT_1C94, Index::new(index.get() as usize).expect("result index")).unwrap_or([0; 104]), out),
        }
    }

    fn write_block(&mut self, record: RecordHandle, block: Block, words: &[u32]) {
        macro_rules! record { ($group:ident :: $field:ident) => {{ let _ = self.records.write(record, &r::$group::$field, pack(words)); }}; }
        macro_rules! shared { ($group:ident :: $field:ident) => {{ let _ = self.records.write_shared(record, &s::$group::$field, pack(words)); }}; }
        match block {
            Block::Command1f | Block::AuxiliaryWindow => record!(group_13::AT_20B8),
            Block::Prepare26 => shared!(group_37::AT_085C), Block::Suspension => shared!(group_33::AT_07A0),
            Block::Lens(slot) => { let _ = self.records.write_shared_element(record, &s::group_41::AT_0974, lens_index(slot), pack(words)); }
            Block::SelectionRange => record!(group_13::AT_2100), Block::Spans => record!(group_30::AT_17FC),
            Block::Classes => record!(group_00::AT_0004), Block::WindowBytes => record!(group_00::AT_000E),
            Block::ReleaseRange => shared!(group_17::LENS_SNAPSHOT), Block::Options => shared!(group_19::AT_0120),
            Block::LensTarget => shared!(group_26::AT_0240), Block::Release => shared!(group_14::FOCUS_RESULT),
            Block::FocusConfirmation => shared!(group_15::FOCUS_CONFIRMATION), Block::FocusHistory => shared!(group_25::FOCUS_HISTORY),
            Block::MeasuredWindow => shared!(group_22::AT_0180),
            Block::ResultRecord(index) => { let _ = self.records.write_element(record, &r::group_30::AT_1C94, Index::new(index.get() as usize).expect("result index"), pack(words)); }
        }
    }

    fn clear_selection(&mut self, record: RecordHandle) { let _ = self.records.write_shared(record, &s::group_29::AT_079C, 0); }
    fn select_descriptor(&mut self, record: RecordHandle, descriptor: u32) { let _ = self.records.write(record, &r::group_04::AT_0160_4, descriptor); }
    fn set_descriptor_pair(&mut self, record: RecordHandle, first: u32, second: u32) {
        let _ = self.records.write(record, &r::group_04::AT_0164, first);
        let _ = self.records.write(record, &r::group_04::AT_0168, second);
    }
    fn notify_selection_owner(&mut self, record: RecordHandle) {
        let callback = self.shared(record, &s::group_20::AT_016C);
        if callback != 0 { let _ = self.machine.enter(callback, &[0, 0, 0, 0, 1]); }
    }
}
