use crate::boundary::{ClientRef, FocusResult, Frame, LensReference};
use crate::calculations_work::Processing;
use crate::capture_boundary::*;
use crate::capture_control::CaptureControl;
use crate::capture_dump;
use crate::dump_boundary::*;
use crate::primary_job::PrimaryJob;
use crate::secondary_boundary::MeasurementCalc;
use crate::subjects::SubjectList;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::{stock, Completion, RecordHandle};

const FRAME: Frame = Frame(0x8000_0000);
const MCTS: u32 = 0x5354_434d;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x2000) }

fn parameters(mode: u32) -> CaptureParameters {
    CaptureParameters {
        options: CaptureOptions(0),
        mode,
        sources: ClientRef(0x200),
        profile: ClientRef(0x280),
        slots: ClientRef(0x300),
        selection: ClientRef(0x400),
        calibration: ClientRef(0),
        window: ClientRef(0),
        areas: ClientRef(0),
        statistics_selector: ClientRef(0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    Word(u32),
    Copy(FrameOffset, FrameTable),
    Export(Frame, u32),
}

pub(super) struct World {
    diagnostics: u8,
    fields: BTreeMap<u8, u32>,
    slot_fields: BTreeMap<u8, u32>,
    area: AreaReference,
    subject_mode: u32,
    marker: i16,
    adjustments: BTreeMap<u32, u8>,
    window: Option<[u32; 2]>,
    window_requests: Vec<(u32, [u16; 2])>,
    remapped: i32,
    remaps: Vec<RemapRequest>,
    frame: BTreeMap<u32, u32>,
    writes: Vec<u32>,
    events: Vec<Event>,
}

impl World {
    pub(super) fn new() -> Self {
        Self {
            diagnostics: 0x80,
            fields: BTreeMap::new(),
            slot_fields: BTreeMap::new(),
            area: AreaReference([0; 10]),
            subject_mode: 0,
            marker: 0,
            adjustments: BTreeMap::new(),
            window: None,
            window_requests: Vec::new(),
            remapped: 0,
            remaps: Vec::new(),
            frame: BTreeMap::new(),
            writes: Vec::new(),
            events: Vec::new(),
        }
    }

    fn set(&mut self, field: RecordField, value: u32) { self.fields.insert(field as u8, value); }

    fn set_slot(&mut self, field: SlotField, value: u32) { self.slot_fields.insert(field as u8, value); }

    fn adjust(&mut self, key: u32, bytes: &[u8]) {
        for (index, byte) in (0..).zip(bytes) { self.adjustments.insert(key + index, *byte); }
    }

    fn at(&self, offset: u32) -> Option<u32> { self.frame.get(&offset).copied() }

    fn finalize(&mut self, mode: u32) {
        let mut control = CaptureControl::new();
        control.bind(record());
        control.finalize(&parameters(mode), &PrimaryJob::new(), &mut Shot, self);
    }

    fn write(&mut self, output: u32) { capture_dump::write(record(), &parameters(0), output, &mut Shot, self); }
}

fn workspace_value(workspace: Workspace, offset: u32) -> u32 { ((workspace as u32 + 1) << 24) | offset }

impl DumpWorkspaces for World {
    fn workspace_field(&mut self, workspace: Workspace, field: WorkspaceField) -> u32 { workspace_value(workspace, field.offset) }
}

impl DumpSettings for World {
    fn adjustment_byte(&mut self, key: AdjustmentKey) -> u8 { self.adjustments.get(&key.0).copied().unwrap_or(0) }
    fn settings_class(&mut self) -> u32 { 3 }
    fn export_configuration(&mut self, frame: Frame, distance: u32) { self.events.push(Event::Export(frame, distance)); }
}

impl DumpShared for World {
    fn capture_window(&mut self, mode: u32, layout: [u16; 2]) -> Option<[u32; 2]> {
        self.window_requests.push((mode, layout));
        self.window
    }
}

/// Shot words carry their name's index.
struct Shot;

impl crate::capture_dump::DumpShot for Shot {
    fn shot_word(&mut self, word: ShotWord) -> u32 { 0x5000 + word as u32 }
}

impl CaptureClient for World {
    fn frame(&mut self, sources: ClientRef) -> Frame {
        assert_eq!(sources, ClientRef(0x200));
        FRAME
    }
    fn frame_detail(&mut self, _sources: ClientRef) -> u32 { 0 }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection { SlotSelection::default() }
    fn slot(&mut self, _slots: ClientRef, _index: u8) -> SlotHandle { SlotHandle(0) }
    fn store_selection_result(&mut self, _selection: ClientRef, _value: u32) {}
    fn store_selection_hook(&mut self, _selection: ClientRef) {}
    fn output_marker(&mut self, selection: ClientRef) -> i16 {
        assert_eq!(selection, ClientRef(0x400));
        self.marker
    }
    fn frame_word(&mut self, _frame: Frame, offset: FrameOffset) -> u32 { self.at(offset.0).unwrap_or(0) }
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32) {
        assert_eq!(frame, FRAME);
        self.frame.insert(offset.0, value);
        self.writes.push(offset.0);
        self.events.push(Event::Word(offset.0));
    }
    fn fill_frame(&mut self, _frame: Frame, _offset: FrameOffset, _bytes: u32, _value: u8) {}
    fn copy_table(&mut self, frame: Frame, offset: FrameOffset, table: FrameTable) {
        assert_eq!(frame, FRAME);
        self.events.push(Event::Copy(offset, table));
    }
}

impl CaptureRecords for World {
    fn field(&mut self, record: RecordHandle, field: RecordField) -> u32 {
        assert_eq!(record, self::record());
        self.fields.get(&(field as u8)).copied().unwrap_or(0)
    }
    fn set_field(&mut self, _record: RecordHandle, _field: RecordField, _value: u32) {}
    fn slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField) -> u32 {
        assert_eq!(slot, 0);
        self.slot_fields.get(&(field as u8)).copied().unwrap_or(0)
    }
    fn set_slot_field(&mut self, _record: RecordHandle, _slot: u8, _field: SlotField, _value: u32) {}
    fn reference_pair(&mut self, _record: RecordHandle, _index: u8) -> ReferencePair { ReferencePair::default() }
    fn slot_entry(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry {
        assert_eq!(slot, 0);
        let mut entry = SlotEntry([0; SlotEntry::BYTES]);
        entry.0[0xa0..0xa4].copy_from_slice(&self.subject_mode.to_le_bytes());
        entry
    }
    fn store_slot_entry(&mut self, _target: SlotHandle, _entry: &SlotEntry) {}
    fn slot_reference(&mut self, _slot: SlotHandle) -> u32 { 0 }
    fn slot_offset(&mut self, _slot: SlotHandle) -> i16 { 0 }
    fn load_profile(&mut self, _record: RecordHandle, _profile: ClientRef) {}
    fn select_slot(&mut self, _record: RecordHandle, _slot: SlotHandle) {}
    fn focus_result(&mut self, _record: RecordHandle) -> FocusResult {
        FocusResult { reference: LensReference(0), valid: 0, kept: 0, refined: 0, extent: 0, bytes: [0; 132] }
    }
    fn store_focus_result(&mut self, _record: RecordHandle, _result: &FocusResult) {}
    fn area_reference(&mut self, _record: RecordHandle) -> AreaReference { self.area }
    fn store_setting_result(&mut self, _record: RecordHandle, _result: &SettingResult) {}
    fn store_item(&mut self, _record: RecordHandle, _index: u8, _item: &AreaEntry) {}
    fn publish_subjects(&mut self, _record: RecordHandle, _list: &SubjectList) {}
}

impl LensGeometry for World {
    fn reference_offset(&mut self, _reference: u32) -> i16 { 0 }
    fn rounded(&mut self, _value: u32) -> i16 { 0 }
    fn convert(&mut self, _base: u32, value: u32, _lens: u32) -> u32 { value }
    fn mapped(&mut self, value: u32, _selector: u32) -> u32 { value }
    fn compensated(&mut self, _reference: u32, _lens: u32) -> i16 { 0 }
    fn remap(&mut self, request: RemapRequest) -> i32 {
        self.remaps.push(request);
        self.remapped
    }
}

impl MeasurementCalc for World {
    fn capture_selection(&mut self, _record: RecordHandle) -> CaptureSelection { CaptureSelection::default() }
    fn statistics_key(&mut self, _processing: &Processing) -> [i16; 2] { [0; 2] }
    fn scale_step(&mut self, _index: u16) -> i16 { 0 }
    fn capture_offset(&mut self, record: RecordHandle) -> u32 {
        assert_eq!(record, self::record());
        0xc0ff
    }
    fn curve_within(&mut self, _distance: u32, _selection: u32, _drive: u32, _limit: i32) -> bool { false }
    fn request_evaluation(&mut self, _record: RecordHandle, _processing: &Processing, _frames: [u32; 2], _geometry: &AreaGeometry, _completions: [Completion; 2]) {}
}

impl CapturePlatform for World {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 {
        assert_eq!(adjustment, CaptureAdjustment::Diagnostics);
        self.diagnostics
    }
    fn prepare_setting(&mut self) {}
    fn refresh_setting(&mut self) {}
    fn monitor_device_commands(&mut self, _monitored: bool) {}
    fn device_status_bit(&mut self) -> bool { false }
    fn slot_rate(&mut self, _active: u32, _profile: u32) -> u32 { 0 }
    fn split_distance(&mut self, _distance: i16) -> DistanceSplit { DistanceSplit::default() }
    fn device_value(&mut self) -> u32 { 0 }
    fn export_record_snapshot(&mut self, _frame: Frame) {}
    fn export_pipeline(&mut self, _frame: Frame) {}
    fn prepare_dump_area(&mut self, _frame: Frame, _offset: FrameOffset) {}
}

#[test]
fn a_finished_capture_dumps_only_when_the_diagnostics_ask_and_the_capture_is_not_reduced() {
    let mut world = World::new();
    world.diagnostics = 0x7f;
    world.finalize(0);
    assert!(world.events.is_empty());

    let mut world = World::new();
    world.finalize(CaptureParameters::REDUCED_MODE);
    assert!(world.events.is_empty());

    let mut world = World::new();
    CaptureControl::new().finalize(&parameters(0), &PrimaryJob::new(), &mut Shot, &mut world);
    assert!(world.events.is_empty(), "nothing is dumped before a record is bound");

    let mut world = World::new();
    world.finalize(0);
    assert_eq!(world.at(0), Some(MCTS));
}

#[test]
fn the_first_part_writes_every_word_once_but_four() {
    let mut world = World::new();
    world.write(0);
    let first: Vec<u32> = world.writes.iter().copied().filter(|offset| *offset < 0x1e00).collect();
    let mut distinct = first.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), first.len(), "no word of the first part is written twice");
    let skipped: Vec<u32> = (0..0x1e00).step_by(4).filter(|offset| !distinct.contains(offset)).collect();
    assert_eq!(skipped, [0xe4, 0xe8, 0xec, 0xf0]);
    assert_eq!(world.writes[0], 0, "the start mark comes first");
}

#[test]
fn the_record_part_carries_record_and_slot_fields_between_its_marks() {
    let mut world = World::new();
    world.set(RecordField::SubjectSource, 0x31);
    world.set(RecordField::SecondaryMode, 2);
    world.set(RecordField::ReferenceOption, 0x48);
    world.set(RecordField::At0039, 0x39);
    world.set(RecordField::SubjectsLocked, 5);
    world.set(RecordField::SubjectKind, 0x42);
    world.set(RecordField::SubjectArea, 0x63);
    world.set(RecordField::IndexedSelection, 0x2714);
    world.set_slot(SlotField::At0182, 0xffff_fff0);
    world.set_slot(SlotField::Reference, 0x2a40);
    world.set_slot(SlotField::Distance, 0x4300);
    world.set_slot(SlotField::At0204, 0x43b0);
    world.write(0);
    let words = [
        (0x004, 0x31), (0x034, 2), (0x048, 0x48), (0x070, 0xffff_fff0), (0x074, 0), (0x078, 0x2a40), (0x090, 0x4300), (0x094, 0x2714),
        (0x098, 0x2714), (0x09c, 0xc0ff), (0x0a8, 0x43b0), (0x0c8, 0), (0x0cc, 0), (0x0d8, 0), (0x0dc, 0x39), (0x104, 5), (0x108, 0x42),
        (0x10c, 3), (0x110, 0x4c41_434d), (0x114, 0x5354_5352), (0x118, 0x63), (0x1dfc, 0x4c41_5352),
    ];
    for (offset, value) in words { assert_eq!(world.at(offset), Some(value), "frame word {offset:#x}"); }
}

#[test]
fn the_shot_context_and_the_selection_marker_go_into_the_record_part() {
    let mut world = World::new();
    world.marker = -2;
    world.write(0);
    assert_eq!(world.at(0x7c), Some(0xffff_fffe));
    let words = [(0x80, ShotWord::RequestedReference), (0x84, ShotWord::StartReference), (0x88, ShotWord::SecondReference),
        (0x8c, ShotWord::ThirdReference), (0xa0, ShotWord::Moved), (0xa4, ShotWord::MovedLate)];
    for (offset, word) in words { assert_eq!(world.at(offset), Some(0x5000 + word as u32)); }
}

#[test]
fn subject_flags_take_bit_0_from_the_area_reference_when_the_record_names_an_area() {
    let flags = |area: u32, fifth: u32| {
        let mut world = World::new();
        world.set(RecordField::At002e, 0x1_2346);
        world.set(RecordField::SubjectArea, area);
        world.area.0[4] = fifth;
        world.write(0);
        world.at(8)
    };
    assert_eq!(flags(0, 1), Some(0x1_2346));
    assert_eq!(flags(2, 1), Some(0x2347));
    assert_eq!(flags(2, 3), Some(0x2346));
}

#[test]
fn the_layout_halves_and_the_capture_window_of_the_subject_mode() {
    let mut world = World::new();
    world.set(RecordField::Layout, 0x0004_0003);
    world.set(RecordField::SubjectsLocked, 6);
    world.window = Some([0x11, 0x22]);
    world.write(0);
    assert_eq!([world.at(0x1c), world.at(0x20), world.at(0x24), world.at(0x28)], [Some(3), Some(4), Some(0x11), Some(0x22)]);
    assert_eq!(world.window_requests, [(6, [3, 4])]);

    let mut world = World::new();
    world.set(RecordField::Layout, 0x0010_0009);
    world.subject_mode = 9;
    world.write(0);
    assert_eq!(world.window_requests, [(9, [9, 16])], "unlocked subjects take the first slot's mode");
    assert_eq!([world.at(0x24), world.at(0x28)], [Some(0), Some(0)], "a layout without a window leaves zeros");
}

#[test]
fn adjustment_bytes_are_packed_into_words() {
    let mut world = World::new();
    world.adjust(0x204, &[0x12, 0x34]);
    world.adjust(0x1ea, &[0x56, 0x78]);
    world.adjust(0x1ec, &[1, 2, 3, 4]);
    world.adjust(0x1f8, &[0xa, 0xb, 0xc, 0xd]);
    world.adjust(0x75b0, &[0x9e]);
    world.adjust(0x75c4, &[7]);
    world.adjust(0x1_8ba9, &[0x34, 0x12]);
    world.write(0);
    let words = [(0xac, 0x1234), (0xb4, 0x56_0078), (0xb8, 0x0102_0304), (0xc4, 0x0a0b_0c0d), (0x11c, 0x9e), (0x350, 70), (0x1cb8, 0x1234)];
    for (offset, value) in words { assert_eq!(world.at(offset), Some(value), "frame word {offset:#x}"); }
}

#[test]
fn list_lengths_are_at_least_one_and_at_most_twenty() {
    for (byte, length) in [(0, 1), (7, 7), (20, 20), (25, 20)] {
        let mut world = World::new();
        world.adjust(0x75b1, &[byte]);
        world.adjust(0x75df, &[byte]);
        world.write(0);
        assert_eq!([world.at(0x1d8), world.at(0x244), world.at(0x1cc8)], [Some(length); 3]);
        assert_eq!([world.at(0x1dc), world.at(0x248), world.at(0x2a4), world.at(0x1ccc)], [Some(20); 4]);
    }
}

#[test]
fn each_grid_position_is_remapped_at_its_reference_and_state() {
    let mut world = World::new();
    world.remapped = 0x0400_0040;
    world.write(0);
    let primary = |offset| workspace_value(Workspace::PrimaryGrid, offset);
    let secondary = |offset| workspace_value(Workspace::SecondaryGrid, offset);
    let request = |grid: &dyn Fn(u32) -> u32| RemapRequest { position: grid(0x30), reference: grid(0x3c), state: grid(0x40), target: 0, remap: false, kind: 1 };
    assert_eq!(world.remaps, [request(&primary), request(&secondary)]);
    assert_eq!(world.at(0x154), Some(0x0400_0040u32.wrapping_sub(primary(0x30))));
    assert_eq!(world.at(0x1a8), Some(0x0400_0040u32.wrapping_sub(secondary(0x30))));
    assert_eq!([world.at(0x158), world.at(0x1ac)], [Some(0); 2]);
    assert_eq!([world.at(0x15c), world.at(0x1b0)], [Some(primary(0x3d)), Some(secondary(0x3d))]);
}

#[test]
fn workspace_fields_land_at_their_frame_words() {
    let mut world = World::new();
    world.write(0);
    let copies = [
        (0x124, Workspace::PrimaryGrid, 0x00),
        (0x13c, Workspace::PrimaryGrid, 0x0a),
        (0x144, Workspace::PrimaryGrid, 0x28),
        (0x174, Workspace::PrimaryGrid, 0x4e),
        (0x198, Workspace::SecondaryGrid, 0x2c),
        (0x1cc, Workspace::SecondaryGrid, 0x44),
        (0x1e0, Workspace::PrimaryKeyDefaults, 0),
        (0x22c, Workspace::PrimaryKeyDefaults, 19),
        (0x24c, Workspace::KeyReferences, 0),
        (0x298, Workspace::KeyReferences, 19 * 0x214),
        (0x2f4, Workspace::TertiaryKeyDefaults, 19),
        (0x324, Workspace::PrimaryGrid, 0xb3),
        (0x33c, Workspace::PrimaryGrid, 0xc8),
        (0x354, Workspace::PrimaryGrid, 0x4ec),
        (0x358, Workspace::PrimaryGrid, 0xd6),
        (0x754, Workspace::PrimaryGrid, 0x2d4),
        (0x758, Workspace::ScaledInputs, 0x218),
        (0xb58, Workspace::ScaledInputs, 0x668),
        (0x1b54, Workspace::ScaledInputs, 4 * 0x450 + 0x416),
        (0x1b74, Workspace::ScaledInputs, 4 * 0x450 + 0x41e),
        (0x1ba4, Workspace::ScaledInputs, 4 * 0x450 + 0x428),
        (0x1ba8, Workspace::KeyIndexes, 4),
        (0x1bf8, Workspace::KeyIndexes, 8),
        (0x1c98, Workspace::KeyIndexes, 19 * 0x24),
        (0x1cb4, Workspace::PrimaryGrid, 0x51c),
        (0x1cc0, Workspace::SecondaryGrid, 0x84),
        (0x1d1c, Workspace::SecondaryKeyDefaults, 19),
        (0x1d2c, Workspace::PrimaryGrid, 0xa4),
        (0x1d34, Workspace::GridParameters, 0x10),
        (0x1d98, Workspace::GridParameters, 0x54),
        (0x1db8, Workspace::GridParameters, 0x50),
        (0x1dec, Workspace::GridLimits, 0x30),
        (0x1df8, Workspace::GridLimits, 0x3c),
    ];
    for (offset, workspace, field) in copies {
        assert_eq!(world.at(offset), Some(workspace_value(workspace, field)), "frame word {offset:#x}");
    }
}

#[test]
fn the_trace_area_is_marked_around_the_primary_output_unless_secondary_mode_is_6() {
    let mut world = World::new();
    world.write(0xfeed);
    assert_eq!([world.at(0xa_074c), world.at(0xa_567c), world.at(0xa_5adc)], [Some(0x5354_4344), Some(0xfeed), Some(0x4c41_4344)]);

    let mut world = World::new();
    world.set(RecordField::SecondaryMode, 6);
    world.write(0xfeed);
    assert_eq!([world.at(0xa_074c), world.at(0xa_567c), world.at(0xa_5adc)], [None; 3]);
}

#[test]
fn the_configuration_and_the_pipeline_storage_close_the_dump() {
    let mut world = World::new();
    world.set_slot(SlotField::Distance, 0x4300);
    world.write(0);
    let tail: Vec<Event> = world.events.iter().copied().skip_while(|event| *event != Event::Word(0xa_5adc)).skip(1).collect();
    assert_eq!(tail, [
        Event::Export(FRAME, 0x4300),
        Event::Word(0xb_1afc),
        Event::Copy(FrameOffset(0xb_1b00), FrameTable::PipelineWork),
        Event::Word(0xb_6ce0),
    ]);
    assert_eq!([world.at(0xb_1afc), world.at(0xb_6ce0)], [Some(0x5354_5447), Some(0x4c41_5447)]);
}
