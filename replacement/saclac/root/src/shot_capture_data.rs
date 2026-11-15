#![forbid(unsafe_code)]

use xm1_saclac_interface::{area_measurement::AreaEntry, calc::SharedAddress, capture_job::{CaptureEnd, OutputSelection}, RecordHandle};
use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, CopyGranule, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{ClientRef, FocusResult, Frame}, capture_boundary::{AreaReference, CaptureClient, CaptureRecords, FrameOffset, FrameTable, RecordField, ReferencePair, SettingResult, SlotEntry, SlotField, SlotHandle, SlotSelection}, capture_stock as layout, primary_boundary::{Calibration, CaptureJobClient, CaptureJobRecords, ClientCalibration, DeliveredList, SlotPosition, SlotPositions, SourceWords}, stock as shot, subjects::SubjectList};
use crate::{shot_services::Libraries, stock::acquisition::{read_record, write_record}};

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().expect("capture word")) }
fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().expect("capture halfword")) }
fn read_word(machine: &impl Machine, at: u32) -> u32 {
    if at & 3 == 0 { machine.read_u32(at) } else { u32::from_le_bytes(read_record(machine, SharedAddress(at))) }
}
fn read_half(machine: &impl Machine, at: u32) -> u16 {
    if at & 1 == 0 { machine.read_u16(at) } else { u16::from_le_bytes(read_record(machine, SharedAddress(at))) }
}
fn write_word(machine: &mut impl Machine, at: u32, value: u32) {
    if at & 3 == 0 { machine.write_u32(at, value); } else { write_record(machine, SharedAddress(at), 0, &value.to_le_bytes()); }
}
fn write_half(machine: &mut impl Machine, at: u32, value: u16) {
    if at & 1 == 0 { machine.write_u16(at, value); } else { write_record(machine, SharedAddress(at), 0, &value.to_le_bytes()); }
}
fn list_offset(list: DeliveredList) -> u32 {
    layout::AREA_BUFFER_LISTS[match list { DeliveredList::First => 0, DeliveredList::Second => 1, DeliveredList::Third => 2 }]
}

struct ProfileSource(u32);
impl ProfileSource {
    fn new(source: ClientRef) -> Self {
        assert_eq!(source.0 & 3, 0, "profile copy source alignment");
        Self(source.0)
    }
    fn word(&self, machine: &impl Machine, offset: Index<56>, index: usize) -> u32 {
        machine.read_u32(self.0.wrapping_add(offset.get() as u32).wrapping_add(4 * index as u32))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureClient for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn frame(&mut self, sources: ClientRef) -> Frame { Frame(read_word(&self.services.borrow().machine, sources.0.wrapping_add(layout::SOURCE_FRAME))) }
    fn frame_detail(&mut self, sources: ClientRef) -> u32 { read_word(&self.services.borrow().machine, sources.0.wrapping_add(layout::SOURCE_FRAME_DETAIL)) }
    fn selection(&mut self, selection: ClientRef) -> SlotSelection {
        let services = self.services.borrow();
        SlotSelection { requested: services.machine.read_u8(selection.0), count: services.machine.read_u8(selection.0.wrapping_add(1)) }
    }
    fn slot(&mut self, slots: ClientRef, index: u8) -> SlotHandle { SlotHandle(read_word(&self.services.borrow().machine, slots.0.wrapping_add(u32::from(index) * 4))) }
    fn store_selection_result(&mut self, selection: ClientRef, value: u32) { write_word(&mut self.services.borrow().machine.clone(), selection.0.wrapping_add(layout::SELECTION_RESULT), value); }
    fn store_selection_hook(&mut self, selection: ClientRef) {
        write_word(&mut self.services.borrow().machine.clone(), selection.0.wrapping_add(layout::SELECTION_HOOK), self.callbacks.capture_selection_hook.0);
    }
    fn output_marker(&mut self, selection: ClientRef) -> i16 { read_half(&self.services.borrow().machine, selection.0.wrapping_add(layout::OUTPUT_MARKER)) as i16 }
    fn frame_word(&mut self, frame: Frame, offset: FrameOffset) -> u32 { read_word(&self.services.borrow().machine, frame.0.wrapping_add(offset.0)) }
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32) { write_word(&mut self.services.borrow().machine.clone(), frame.0.wrapping_add(offset.0), value); }
    fn fill_frame(&mut self, frame: Frame, offset: FrameOffset, bytes: u32, value: u8) {
        let mut machine = self.services.borrow().machine.clone();
        for index in 0..bytes { machine.write_u8(frame.0.wrapping_add(offset.0).wrapping_add(index), value); }
    }
    fn copy_table(&mut self, frame: Frame, offset: FrameOffset, table: FrameTable) {
        let mut machine = self.services.borrow().machine.clone();
        let (source, bytes) = match table {
            FrameTable::SelectionMap => (layout::SELECTION_MAP_TABLE, layout::MAP_TABLE_BYTES),
            FrameTable::PipelineWork => (0x016f_8f30, layout::PIPELINE_WORK_BYTES),
        };
        for index in 0..bytes {
            let byte = machine.read_u8(source.wrapping_add(index));
            machine.write_u8(frame.0.wrapping_add(offset.0).wrapping_add(index), byte);
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureJobClient for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn source_words(&mut self, sources: ClientRef) -> SourceWords {
        let services = self.services.borrow();
        let at = |offset| read_word(&services.machine, sources.0.wrapping_add(offset));
        SourceWords { first: at(0), words: [at(0xc), at(0x14), at(0x20)], byte: services.machine.read_u8(sources.0.wrapping_add(0x3c)), option: at(0x4c) }
    }
    fn store_source_output(&mut self, sources: ClientRef, marker: u16, value: u32) {
        let mut machine = self.services.borrow().machine.clone();
        write_half(&mut machine, sources.0.wrapping_add(0x42), marker);
        write_word(&mut machine, sources.0.wrapping_add(0x44), value);
    }
    fn selection_option(&mut self, selection: ClientRef) -> u8 { self.services.borrow().machine.read_u8(selection.0.wrapping_add(4)) }
    fn readout_mode(&mut self, selection: ClientRef) -> u32 { read_word(&self.services.borrow().machine, selection.0.wrapping_add(0x18)) }
    fn store_output(&mut self, selection: ClientRef, output: &OutputSelection, offset: u32, rate: u32) {
        let mut machine = self.services.borrow().machine.clone();
        write_half(&mut machine, selection.0.wrapping_add(8), output.marker);
        write_word(&mut machine, selection.0.wrapping_add(0xc), output.word);
        write_half(&mut machine, selection.0.wrapping_add(0x10), output.pair);
        machine.write_u8(selection.0.wrapping_add(0x12), output.byte);
        write_half(&mut machine, selection.0.wrapping_add(0x14), output.pair);
        for (at, value) in [(0x1c, output.words[0]), (0x20, output.words[1]), (0x24, output.words[2]), (0x40, offset), (0x44, rate)] { write_word(&mut machine, selection.0.wrapping_add(at), value); }
    }
    fn store_capture_end(&mut self, selection: ClientRef, end: &CaptureEnd) {
        let mut machine = self.services.borrow().machine.clone();
        for (at, value) in [(0x18, end.readout[0]), (0x1c, end.readout[1]), (0x24, end.detail)] { write_word(&mut machine, selection.0.wrapping_add(at), value); }
    }
    fn calibration_block(&mut self, block: ClientRef) -> ClientCalibration {
        let services = self.services.borrow();
        ClientCalibration { word: read_word(&services.machine, block.0), calibration: Calibration(core::array::from_fn(|i| read_half(&services.machine, block.0.wrapping_add(4 + 2 * i as u32)))), rate: services.machine.read_u8(block.0.wrapping_add(0x16)) }
    }
    fn statistics_window(&mut self, block: ClientRef) -> [u32; 4] { core::array::from_fn(|i| read_word(&self.services.borrow().machine, block.0.wrapping_add(4 * i as u32))) }
    fn statistics_selector(&mut self, block: ClientRef) -> u32 { read_word(&self.services.borrow().machine, block.0.wrapping_add(4)) }
    fn clear_area_list(&mut self, buffer: ClientRef, list: DeliveredList) {
        let mut machine = self.services.borrow().machine.clone();
        for offset in 0..0x414 { machine.write_u8(buffer.0.wrapping_add(list_offset(list)).wrapping_add(offset), 0); }
    }
    fn area_count(&mut self, buffer: ClientRef, list: DeliveredList) -> u32 { read_word(&self.services.borrow().machine, buffer.0.wrapping_add(list_offset(list))) }
    fn area(&mut self, buffer: ClientRef, list: DeliveredList, index: u8) -> AreaEntry {
        assert!(u32::from(index) < layout::AREA_BUFFER_CAPACITY, "capture client area index");
        AreaEntry(read_record(&self.services.borrow().machine, SharedAddress(buffer.0.wrapping_add(list_offset(list)).wrapping_add(4 + 104 * u32::from(index)))))
    }
    fn set_area(&mut self, buffer: ClientRef, list: DeliveredList, index: u8, area: &AreaEntry) {
        if u32::from(index) < layout::AREA_BUFFER_CAPACITY { write_record(&mut self.services.borrow().machine.clone(), SharedAddress(buffer.0.wrapping_add(list_offset(list)).wrapping_add(4 + 104 * u32::from(index))), 0, &area.0); }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn field(&mut self, record: RecordHandle, field: RecordField) -> u32 {
        let mut services = self.services.borrow_mut();
        macro_rules! get { ($group:ident::$name:ident) => { services.field(record, &r::$group::$name) as u32 }; }
        macro_rules! shared { ($group:ident::$name:ident) => { services.shared(record, &s::$group::$name) as u32 }; }
        match field {
            RecordField::ResourceKind => get!(group_00::AT_0032),
            RecordField::RemapMode => get!(group_00::AT_0064),
            RecordField::ReferenceFirstLimit => get!(group_00::AT_001C),
            RecordField::ReferenceSecondLimit => get!(group_00::AT_001E),
            RecordField::ReferenceOption => get!(group_00::AT_0044),
            RecordField::CaptureFrame => shared!(group_42::AT_09C8),
            RecordField::CaptureSlotCount => shared!(group_42::AT_09CC),
            RecordField::CaptureSlots => shared!(group_42::AT_09D0),
            RecordField::SlotMode => shared!(group_38::AT_0894),
            RecordField::SubjectKind => get!(group_00::AT_0051),
            RecordField::SubjectSource => u32::from(services.entries.header_byte(record, SharedHeaderByte::At31)),
            RecordField::SubjectArea => u32::from(services.entries.header_byte(record, SharedHeaderByte::At63)),
            RecordField::SubjectsLocked => get!(group_00::AT_0034),
            RecordField::KeepsAllSubjects => u32::from(services.entries.header_byte(record, SharedHeaderByte::At61)),
            RecordField::SlotLock | RecordField::SecondaryLatch => get!(group_00::AT_0041),
            RecordField::SecondaryMode | RecordField::SecondaryStyle => get!(group_00::AT_003F),
            RecordField::SecondaryBlocked => get!(group_00::AT_0040),
            RecordField::JobPrepared => shared!(group_14::AT_0890),
            RecordField::MapPresent => u32::from(services.field(record, &r::group_00::INTERVAL_TRACKING).0),
            RecordField::Layout => get!(group_00::AT_0000),
            RecordField::SecondaryOption => get!(group_00::AT_0048),
            RecordField::ProfileClass => shared!(group_27::AT_0740),
            RecordField::FrameFirst => get!(group_04::AT_0164),
            RecordField::FrameSecond => get!(group_04::AT_0168),
            RecordField::PlacesItem => get!(group_00::AT_005D),
            RecordField::MeasurementOption => u32::from(services.entries.header_byte(record, SharedHeaderByte::At46)),
            RecordField::StatisticsWord | RecordField::IndexedSelection => services.records.chosen_word(record).unwrap_or(0),
            RecordField::StatisticsByte => get!(group_00::AT_0059),
            RecordField::At0014 => get!(group_00::AT_0014),
            RecordField::At001a => get!(group_00::AT_001A),
            RecordField::At0022 => get!(group_00::AT_0022),
            RecordField::At0026 => get!(group_00::AT_0026),
            RecordField::At002e => get!(group_00::AT_002E),
            RecordField::At0033 => get!(group_00::AT_0033),
            RecordField::At0035 => get!(group_00::AT_0035),
            RecordField::At0036 => get!(group_00::AT_0036),
            RecordField::At0037 => get!(group_00::AT_0037),
            RecordField::At0038 => get!(group_00::AT_0038),
            RecordField::At0039 => get!(group_00::AT_0039),
            RecordField::At003a => get!(group_00::AT_003A),
            RecordField::At003b => get!(group_00::AT_003B),
            RecordField::At003c => get!(group_00::AT_003C),
            RecordField::At003d => get!(group_00::AT_003D),
            RecordField::At003e => get!(group_00::AT_003E),
            RecordField::At0042 => get!(group_00::AT_0042),
            RecordField::At0043 => get!(group_00::AT_0043),
            RecordField::At0045 => get!(group_00::AT_0045),
            RecordField::At0047 => u32::from(services.entries.header_byte(record, SharedHeaderByte::At47)),
            RecordField::At0049 => get!(group_00::AT_0049),
            RecordField::At004a => get!(group_00::AT_004A),
            RecordField::At004b => get!(group_00::AT_004B),
            RecordField::At004d => get!(group_00::AT_004D),
            RecordField::At0053 => get!(group_00::AT_0053),
            RecordField::At005b => get!(group_00::AT_005B),
            RecordField::At006c => get!(group_00::AT_006C),
            RecordField::At20b0 => get!(group_13::AT_20B0_4),
            RecordField::At20b4 => get!(group_13::AT_20B4),
            RecordField::At20c8 => get!(group_13::AT_20C8),
            RecordField::At20cc => get!(group_13::AT_20CC),
            RecordField::At20d4 => get!(group_13::AT_20D4),
        }
    }
    fn set_field(&mut self, record: RecordHandle, field: RecordField, value: u32) {
        let mut services = self.services.borrow_mut();
        match field {
            RecordField::CaptureFrame => { let _ = services.records.write_shared(record, &s::group_42::AT_09C8, value); }
            RecordField::CaptureSlotCount => { let _ = services.records.write_shared(record, &s::group_42::AT_09CC, value as u8); }
            RecordField::CaptureSlots => { let _ = services.records.write_shared(record, &s::group_42::AT_09D0, value); }
            RecordField::SlotLock => { let _ = services.records.write(record, &r::group_00::AT_0041, value as u8); }
            RecordField::SecondaryStyle => { let _ = services.records.write(record, &r::group_00::AT_003F, value as u8); }
            RecordField::JobPrepared => { let _ = services.records.write_shared(record, &s::group_14::AT_0890, value); }
            _ => panic!("capture field has no writer"),
        }
    }
    fn slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField) -> u32 {
        let index = Index::new(usize::from(slot)).expect("capture record slot");
        let services = self.services.borrow();
        macro_rules! get { ($group:ident::$name:ident) => { services.records.read_element(record, &r::$group::$name, index).unwrap_or_default() as u32 }; }
        match field {
            SlotField::Reference => get!(group_05::AT_0170),
            SlotField::Offset => get!(group_05::AT_0174),
            SlotField::Rate => get!(group_08::AT_01F4),
            SlotField::Step => get!(group_05::AT_019C),
            SlotField::Residual => get!(group_05::AT_019A),
            SlotField::State => get!(group_07::AT_01C0),
            SlotField::SecondaryFirst => get!(group_07::AT_01B0),
            SlotField::SecondarySecond => get!(group_07::AT_01B4),
            SlotField::DeviceValue => get!(group_07::AT_01EC),
            SlotField::SecondLimit => get!(group_05::AT_0176),
            SlotField::ItemState => services.field(record, &r::group_30::AT_0FC8),
            SlotField::MeasuresAreas => get!(group_08::AT_01FC),
            SlotField::Distance => get!(group_08::AT_0200),
            SlotField::Trim => get!(group_05::AT_0178),
            SlotField::ProfileSelector => get!(group_05::AT_0194),
            SlotField::At017a => get!(group_05::AT_017A),
            SlotField::At0182 => get!(group_05::AT_0182),
            SlotField::At0184 => get!(group_05::AT_0184),
            SlotField::At0186 => get!(group_05::AT_0186),
            SlotField::At0188 => get!(group_05::AT_0188),
            SlotField::At0198 => get!(group_05::AT_0198),
            SlotField::At0204 => get!(group_08::AT_0204),
            SlotField::At0208 => get!(group_10::AT_0208),
            _ => panic!("capture slot field has no reader"),
        }
    }
    fn set_slot_field(&mut self, record: RecordHandle, slot: u8, field: SlotField, value: u32) {
        let Some(index) = Index::new(usize::from(slot)) else { return };
        let mut services = self.services.borrow_mut();
        macro_rules! put { ($group:ident::$name:ident) => {{ let _ = services.records.write_element(record, &r::$group::$name, index, value as _); }}; }
        match field {
            SlotField::Reference => put!(group_05::AT_0170),
            SlotField::Offset => put!(group_05::AT_0174),
            SlotField::Rate => put!(group_08::AT_01F4),
            SlotField::Direction => put!(group_08::AT_01F8),
            SlotField::Step => put!(group_05::AT_019C),
            SlotField::Residual => put!(group_05::AT_019A),
            SlotField::State => put!(group_07::AT_01C0),
            SlotField::DeviceValue => put!(group_07::AT_01EC),
            SlotField::SecondLimit => put!(group_05::AT_0176),
            SlotField::ItemState => { let _ = services.records.write(record, &r::group_30::AT_0FC8, value); }
            SlotField::MeasuredValue => put!(group_07::AT_01BC),
            SlotField::EvaluationWord => put!(group_07::AT_01E8),
            SlotField::EvaluationOffset => put!(group_07::AT_01E4),
            SlotField::Trim => put!(group_05::AT_0178),
            SlotField::Distance => put!(group_08::AT_0200),
            SlotField::At0184 => put!(group_05::AT_0184),
            SlotField::At0186 => put!(group_05::AT_0186),
            SlotField::At01b8 => put!(group_07::AT_01B8),
            SlotField::At0204 => put!(group_08::AT_0204),
            _ => panic!("capture slot field has no writer"),
        }
    }
    fn reference_pair(&mut self, record: RecordHandle, index: u8) -> ReferencePair {
        let index = Index::new(usize::from(index)).expect("capture reference pair");
        let bytes = self.services.borrow().records.read_shared_element(record, &s::group_41::AT_0974, index).expect("capture reference pair read");
        ReferencePair { reference: word(&bytes, 0), detail: word(&bytes, 4) }
    }
    fn slot_entry(&mut self, record: RecordHandle, slot: u8) -> SlotEntry {
        let index = Index::new(usize::from(slot)).expect("capture record slot");
        SlotEntry(self.services.borrow().records.read_element(record, &r::group_05::AT_016C_168, index).expect("capture slot read"))
    }
    fn store_slot_entry(&mut self, target: SlotHandle, entry: &SlotEntry) { write_record(&mut self.services.borrow().machine.clone(), SharedAddress(target.0), 0, &entry.0); }
    fn slot_reference(&mut self, slot: SlotHandle) -> u32 { read_word(&self.services.borrow().machine, slot.0.wrapping_add(4)) }
    fn slot_offset(&mut self, slot: SlotHandle) -> i16 { read_half(&self.services.borrow().machine, slot.0.wrapping_add(8)) as i16 }
    fn load_profile(&mut self, record: RecordHandle, profile: ClientRef) {
        let mut services = self.services.borrow_mut();
        let machine = services.machine.clone();
        let mut source = None;
        let _ = services.records.copy_with_source(record, &r::group_01::AT_0090_56, CopyGranule::SixteenBytes, |offset, out| {
            let source = source.get_or_insert_with(|| ProfileSource::new(profile));
            for (index, chunk) in out.chunks_exact_mut(4).enumerate() { chunk.copy_from_slice(&source.word(&machine, offset, index).to_le_bytes()); }
        });
    }
    fn select_slot(&mut self, record: RecordHandle, slot: SlotHandle) {
        let mut services = self.services.borrow_mut();
        let machine = services.machine.clone();
        let _ = services.records.copy_element_with_source(record, &r::group_05::AT_016C_168, Index::new(0).expect("first slot"), CopyGranule::Byte, |offset, out| {
            out[0] = machine.read_u8(slot.0.wrapping_add(offset.get() as u32));
        });
    }
    fn focus_result(&mut self, record: RecordHandle) -> FocusResult { shot::focus_result(&self.services.borrow().shared_bytes(record, &s::group_14::FOCUS_RESULT)) }
    fn store_focus_result(&mut self, record: RecordHandle, result: &FocusResult) { let _ = self.services.borrow_mut().records.write_shared(record, &s::group_14::FOCUS_RESULT, shot::focus_result_bytes(result)); }
    fn area_reference(&mut self, record: RecordHandle) -> AreaReference {
        let bytes = self.services.borrow().shared_bytes(record, &s::group_27::AT_07EC);
        AreaReference(core::array::from_fn(|i| word(&bytes, i * 4)))
    }
    fn store_setting_result(&mut self, record: RecordHandle, result: &SettingResult) {
        let mut bytes = [0; 20];
        for (chunk, value) in bytes.chunks_exact_mut(4).zip(result.0) { chunk.copy_from_slice(&value.to_le_bytes()); }
        let _ = self.services.borrow_mut().records.write(record, &r::group_32::AT_2140, bytes);
    }
    fn store_item(&mut self, record: RecordHandle, index: u8, item: &AreaEntry) {
        if let Some(index) = Index::new(usize::from(index)) { let _ = self.services.borrow_mut().records.write_element(record, &r::group_30::AT_0FCC, index, item.0); }
    }
    fn publish_subjects(&mut self, record: RecordHandle, list: &SubjectList) {
        // f1dd60 passes fourth-list entry 0 at context +0xc40.
        let _ = self.services.borrow_mut().records.write(record, &r::group_30::AT_17FC, shot::subject_bytes(&list.entries[0]));
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureJobRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn record_calibration(&mut self, record: RecordHandle) -> Calibration {
        let bytes = self.services.borrow().shared_bytes(record, &s::group_22::AT_0180);
        Calibration(core::array::from_fn(|i| half(&bytes, 6 + i * 2)))
    }
    fn statistics_option(&mut self, record: RecordHandle) -> (u32, u8) {
        let bytes = self.services.borrow().shared_bytes(record, &s::group_23::AT_0194);
        (word(&bytes, 4), bytes[0xc])
    }
    fn slot_positions(&mut self, record: RecordHandle) -> SlotPositions {
        let bytes = self.services.borrow().shared_bytes(record, &s::group_34::AT_0814);
        SlotPositions { count: bytes[0], positions: core::array::from_fn(|i| SlotPosition { position: half(&bytes, 4 + i * 8), speed: word(&bytes, 8 + i * 8) }) }
    }
    fn setting_result(&mut self, record: RecordHandle) -> SettingResult {
        let bytes = self.services.borrow().field(record, &r::group_32::AT_2140);
        SettingResult(core::array::from_fn(|i| word(&bytes, i * 4)))
    }
    fn slot_record(&mut self, slot: SlotHandle) -> SlotEntry { SlotEntry(read_record(&self.services.borrow().machine, SharedAddress(slot.0))) }
    fn delivered_count(&mut self, record: RecordHandle, list: DeliveredList) -> u32 {
        let field = match list { DeliveredList::First => &r::group_30::AT_038C, DeliveredList::Second => &r::group_30::AT_0BB4, DeliveredList::Third => &r::group_30::AT_13DC };
        self.services.borrow().field(record, field)
    }
    fn delivered_area(&mut self, record: RecordHandle, list: DeliveredList, index: u8) -> AreaEntry {
        let field = match list { DeliveredList::First => &r::group_30::AT_0390, DeliveredList::Second => &r::group_30::AT_0BB8, DeliveredList::Third => &r::group_30::AT_13E0 };
        let index = Index::new(usize::from(index)).expect("capture delivered area index");
        AreaEntry(self.services.borrow().records.read_element(record, field, index).expect("capture delivered area read"))
    }
    fn store_delivered_count(&mut self, record: RecordHandle, list: DeliveredList, count: u32) {
        let field = match list { DeliveredList::First => &r::group_30::AT_038C, DeliveredList::Second => &r::group_30::AT_0BB4, DeliveredList::Third => &r::group_30::AT_13DC };
        let _ = self.services.borrow_mut().records.write(record, field, count);
    }
    fn store_delivered_area(&mut self, record: RecordHandle, list: DeliveredList, index: u8, area: &AreaEntry) {
        let field = match list { DeliveredList::First => &r::group_30::AT_0390, DeliveredList::Second => &r::group_30::AT_0BB8, DeliveredList::Third => &r::group_30::AT_13E0 };
        if let Some(index) = Index::new(usize::from(index)) { let _ = self.services.borrow_mut().records.write_element(record, field, index, area.0); }
    }
}
