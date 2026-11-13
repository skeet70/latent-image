use crate::boundary::{ClientRef, Frame};
use crate::calculations_work::Processing;
use crate::capture_boundary::*;
use crate::capture_stock;
use crate::finish_work::FinishLensValue;
use crate::ports::{SecondaryEffect, SecondaryQuery};
use crate::secondary_boundary::*;
use crate::secondary_job::SecondaryJob;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_calc::geometry::{ClipRule, ClipSettings};
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection, EvaluatedList, MeasurementRequest, TransferRequest};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

const FRAME: Frame = Frame(0x8000_0000);

fn record() -> RecordHandle { stock::record_handle_from_a2(0x2000) }

fn parameters() -> CaptureParameters {
    CaptureParameters {
        options: CaptureOptions(0),
        mode: 0,
        sources: ClientRef(0x100),
        profile: ClientRef(0x200),
        slots: ClientRef(0x300),
        selection: ClientRef(0x400),
        calibration: ClientRef(0),
        window: ClientRef(0),
        areas: ClientRef(0),
        statistics_selector: ClientRef(0),
    }
}

/// A frame of 16 x 16 cells, each 20 high and 30 wide with gaps of 1 and 2, inside margins of 10
/// (top) and 5 (left). Row `r` starts at `10 + 21 r`, column `c` at `5 + 32 c`.
fn layout() -> ProfileGeometry {
    ProfileGeometry { frame: [355, 520], cell: [20, 30], extent: [41, 42], gap: [2, 1], factors: [3, 4] }
}

fn area_in_row(row: u32) -> AreaEntry {
    let mut area = AreaEntry::EMPTY;
    let top = 10 + 21 * row + 2;
    area.set_rectangle([100, top, 16, 20]);
    area.set_centre([110, top + 8]);
    area.set_scale(64);
    area
}

#[derive(Clone, Debug, PartialEq)]
enum Call {
    SetSlotField(SlotField, u32),
    StoreItem(AreaEntry),
    SlotExtent([u32; 4]),
    SlotCells([u32; 4]),
    FrameWord(FrameOffset, u32),
    Statistics(u8, StatisticsStore),
    Measure(MeasurementRequest, [AreaEntry; 3], [Completion; 2]),
    Evaluate([u32; 2], AreaGeometry, [Completion; 2]),
    Transfer(TransferRequest, Completion),
    Release,
}

const CAPTURE_OFFSET: u32 = 1000;

struct World {
    calls: Vec<Call>,
    fields: BTreeMap<u8, u32>,
    slot_fields: BTreeMap<u8, u32>,
    lists: [Vec<AreaEntry>; 2],
    prefers_second: bool,
    settings: BTreeMap<u8, u32>,
    adjustments: BTreeMap<u8, u8>,
    statistics_accepted: bool,
    crops: BTreeMap<(u32, u32, bool), CropGeometry>,
    words: BTreeMap<u8, u32>,
    clip: ClipRule,
}

impl World {
    fn new() -> Self {
        let mut world = Self {
            calls: Vec::new(),
            fields: BTreeMap::new(),
            slot_fields: BTreeMap::new(),
            lists: [Vec::new(), Vec::new()],
            prefers_second: false,
            settings: BTreeMap::new(),
            adjustments: BTreeMap::new(),
            statistics_accepted: true,
            crops: BTreeMap::new(),
            words: BTreeMap::new(),
            clip: ClipRule::Overlap,
        };
        world.slot_fields.insert(SlotField::MeasuresAreas as u8, 1);
        world.setting(MeasurementSetting::Measure, 1);
        world.setting(MeasurementSetting::ShiftLimit, 5);
        world
    }

    fn setting(&mut self, setting: MeasurementSetting, value: u32) { self.settings.insert(setting as u8, value); }

    fn frame_words(&self) -> BTreeMap<u32, u32> {
        self.calls.iter().filter_map(|call| match call { Call::FrameWord(offset, value) => Some((offset.0, *value)), _ => None }).collect()
    }

    fn measured(&self) -> (MeasurementRequest, [AreaEntry; 3]) {
        self.calls.iter().find_map(|call| match call { Call::Measure(request, areas, _) => Some((*request, *areas)), _ => None }).unwrap()
    }

    fn slot_writes(&self, wanted: SlotField) -> Vec<u32> {
        self.calls.iter().filter_map(|call| match call { Call::SetSlotField(field, value) if *field == wanted => Some(*value), _ => None }).collect()
    }
}

impl CaptureRecords for World {
    fn field(&mut self, _record: RecordHandle, field: RecordField) -> u32 { self.fields.get(&(field as u8)).copied().unwrap_or(0) }
    fn set_field(&mut self, _record: RecordHandle, _field: RecordField, _value: u32) {}
    fn slot_field(&mut self, _record: RecordHandle, _slot: u8, field: SlotField) -> u32 { self.slot_fields.get(&(field as u8)).copied().unwrap_or(0) }
    fn set_slot_field(&mut self, _record: RecordHandle, slot: u8, field: SlotField, value: u32) {
        assert_eq!(slot, 0);
        self.calls.push(Call::SetSlotField(field, value));
    }
    fn reference_pair(&mut self, _record: RecordHandle, _index: u8) -> ReferencePair { ReferencePair::default() }
    fn slot_entry(&mut self, _record: RecordHandle, _slot: u8) -> SlotEntry { SlotEntry([0; SlotEntry::BYTES]) }
    fn store_slot_entry(&mut self, _target: SlotHandle, _entry: &SlotEntry) {}
    fn slot_reference(&mut self, _slot: SlotHandle) -> u32 { 0 }
    fn slot_offset(&mut self, _slot: SlotHandle) -> i16 { 0 }
    fn load_profile(&mut self, _record: RecordHandle, _profile: ClientRef) {}
    fn select_slot(&mut self, _record: RecordHandle, _slot: SlotHandle) {}
    fn focus_result(&mut self, _record: RecordHandle) -> crate::boundary::FocusResult {
        crate::boundary::FocusResult { reference: crate::boundary::LensReference(0), valid: 0, kept: 0, refined: 0, extent: 0, bytes: [0; 132] }
    }
    fn store_focus_result(&mut self, _record: RecordHandle, _result: &crate::boundary::FocusResult) {}
    fn area_reference(&mut self, _record: RecordHandle) -> AreaReference { AreaReference::default() }
    fn store_setting_result(&mut self, _record: RecordHandle, _result: &SettingResult) {}
    fn store_item(&mut self, _record: RecordHandle, index: u8, item: &AreaEntry) {
        assert_eq!(index, 0);
        self.calls.push(Call::StoreItem(*item));
    }
    fn publish_subjects(&mut self, _record: RecordHandle, _list: &crate::subjects::SubjectList) {}
}

impl CaptureClient for World {
    fn frame(&mut self, sources: ClientRef) -> Frame {
        assert_eq!(sources, parameters().sources);
        FRAME
    }
    fn frame_detail(&mut self, _sources: ClientRef) -> u32 { 0x77 }
    fn selection(&mut self, _selection: ClientRef) -> SlotSelection { SlotSelection::default() }
    fn slot(&mut self, _slots: ClientRef, _index: u8) -> SlotHandle { SlotHandle(0) }
    fn store_selection_result(&mut self, _selection: ClientRef, _value: u32) {}
    fn store_selection_hook(&mut self, _selection: ClientRef) {}
    fn frame_word(&mut self, _frame: Frame, _offset: FrameOffset) -> u32 { 0 }
    fn set_frame_word(&mut self, frame: Frame, offset: FrameOffset, value: u32) {
        assert_eq!(frame, FRAME);
        self.calls.push(Call::FrameWord(offset, value));
    }
    fn fill_frame(&mut self, _frame: Frame, _offset: FrameOffset, _bytes: u32, _value: u8) {}
    fn copy_table(&mut self, _frame: Frame, _offset: FrameOffset, _table: FrameTable) {}
}

impl CapturePlatform for World {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 { self.adjustments.get(&(adjustment as u8)).copied().unwrap_or(0) }
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

impl AreaLists for World {
    fn area_count(&mut self, _record: RecordHandle, list: AreaList) -> u32 { self.lists[list as usize].len() as u32 }
    fn area(&mut self, _record: RecordHandle, list: AreaList, index: u8) -> AreaEntry { self.lists[list as usize][usize::from(index)] }
    fn list_detail(&mut self, _record: RecordHandle, list: AreaList) -> u32 { 0x50 + list as u32 }
    fn prefers_second_list(&mut self, _record: RecordHandle) -> bool { self.prefers_second }
    fn store_slot_extent(&mut self, _record: RecordHandle, extent: [u32; 4]) { self.calls.push(Call::SlotExtent(extent)); }
    fn store_slot_cells(&mut self, _record: RecordHandle, cells: [u32; 4]) { self.calls.push(Call::SlotCells(cells)); }
}

impl MeasurementCalc for World {
    fn capture_selection(&mut self, _record: RecordHandle) -> CaptureSelection { CaptureSelection { selection: 9, first: 55, second: 3, auxiliary: 4 } }
    fn statistics_key(&mut self, processing: &Processing) -> [i16; 2] { [i16::from(processing.0[0]), -i16::from(processing.0[0])] }
    /// Steps fall from 70 at index 0 to 0 at index 7, so a selection of 55 spans two rows.
    fn scale_step(&mut self, index: u16) -> i16 { (7 - index as i16) * 10 }
    fn capture_offset(&mut self, _record: RecordHandle) -> u32 { CAPTURE_OFFSET }
    fn curve_within(&mut self, distance: u32, selection: u32, drive: u32, limit: i32) -> bool { distance + selection + drive < limit as u32 }
    fn request_evaluation(&mut self, _record: RecordHandle, _processing: &Processing, frames: [u32; 2], geometry: &AreaGeometry, completions: [Completion; 2]) {
        self.calls.push(Call::Evaluate(frames, *geometry, completions));
    }
}

impl MeasurementProfiles for World {
    fn profile_geometry(&mut self, request: &GeometryRequest) -> ProfileGeometry {
        assert_eq!(request.selector, 8);
        layout()
    }
    fn crop_geometry(&mut self, request: &CropRequest) -> CropGeometry {
        self.crops.get(&(request.selector, request.offset, request.full)).copied().unwrap_or_default()
    }
    fn flagged_scale_pair(&mut self, _selector: u32) -> [u32; 2] { [4, 8] }
    fn profile_extent(&mut self, _selector: u32, inset: bool) -> [u32; 4] { if inset { [1, 2, 3, 4] } else { [7, 9, 0, 0] } }
}

impl MeasurementLens for World {
    fn lens(&mut self, value: FinishLensValue) -> u32 {
        match value {
            FinishLensValue::Distance => 30,
            FinishLensValue::Drive => 5,
            FinishLensValue::Position => 100,
            _ => 0,
        }
    }
    fn clamp_step(&mut self, position: i16, _kind: u32, _direction: u32) -> i16 { position - 50 }
    fn reference_value(&mut self, index: i32) -> u32 { if index == 0 { 200 } else { 150 } }
}

impl MeasurementPipeline for World {
    fn prepare_statistics(&mut self, source: u8, store: StatisticsStore, processing: &mut Processing) -> bool {
        self.calls.push(Call::Statistics(source, store));
        processing.0[0] = source;
        processing.0[0x7c] = source;
        self.statistics_accepted
    }
    fn measurement_word(&mut self, word: MeasurementWord) -> u32 { self.words.get(&(word as u8)).copied().unwrap_or(0) }
    fn release_measurement(&mut self) { self.calls.push(Call::Release); }
    fn measure_areas(&mut self, request: &MeasurementRequest, areas: &[AreaEntry; 3], completions: [Completion; 2]) {
        self.calls.push(Call::Measure(*request, *areas, completions));
    }
    fn transfer(&mut self, request: &TransferRequest, completion: Completion) { self.calls.push(Call::Transfer(*request, completion)); }
}

impl MeasurementSettings for World {
    fn measurement_setting(&mut self, setting: MeasurementSetting) -> u32 { self.settings.get(&(setting as u8)).copied().unwrap_or(0) }
}

impl ClipSettings for World {
    fn clip_rule(&mut self) -> ClipRule { self.clip }
    fn minimum_visible_percent(&mut self) -> u32 { 0 }
}

fn begun(world: &mut World) -> SecondaryJob {
    let mut job = SecondaryJob::new();
    for effect in [SecondaryEffect::Begin(record()), SecondaryEffect::ResetCount, SecondaryEffect::Configure] { job.effect(effect, &parameters(), world); }
    job
}

fn requested(world: &mut World) -> SecondaryJob {
    let mut job = begun(world);
    job.effect(SecondaryEffect::Request, &parameters(), world);
    job
}

fn deliver(job: &mut SecondaryJob, completion: Completion, payload: &[u8]) {
    job.receive(Completed { completion, parameters: stock::parameters(payload) });
}

fn evaluation_payload(edit: impl FnOnce(&mut [u8; 84])) -> [u8; 84] {
    let mut payload = [0; 84];
    edit(&mut payload);
    payload
}

fn put_word(payload: &mut [u8], offset: usize, value: u32) { payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

#[test]
fn without_areas_the_measurement_covers_no_area_and_asks_imgdc_with_both_completions() {
    let mut world = World::new();
    requested(&mut world);
    let (request, areas) = world.measured();
    assert_eq!((request.lists, request.area_count, request.shift), ([false, false], 0, 0));
    assert_eq!(request.selection, CaptureSelection { selection: 9, first: 55, second: 3, auxiliary: 4 });
    assert_eq!(areas, [AreaEntry::EMPTY; 3]);
    assert!(world.calls.iter().any(|call| matches!(call, Call::Measure(_, _, [Completion::Image5b, Completion::Image5c]))));
    let words = world.frame_words();
    assert_eq!(words.get(&capture_stock::dump_list_count(0).0), Some(&0));
    assert_eq!(words.get(&capture_stock::DUMP_SELECTED.0), Some(&0));
}

#[test]
fn an_area_inside_the_band_is_measured_where_it_is() {
    let mut world = World::new();
    world.lists[0].push(area_in_row(7));
    requested(&mut world);
    let (request, areas) = world.measured();
    assert_eq!((request.lists, request.list_details, request.area_count, request.shift), ([true, false], [0x50, 0], 1, 0));
    assert_eq!(areas[0], area_in_row(7), "an area inside the band keeps its rectangle");
    let words = world.frame_words();
    assert_eq!(words[&capture_stock::dump_list_count(0).0], 1);
    assert_eq!((words[&capture_stock::dump_list_pair_word(0, 0).0], words[&capture_stock::dump_list_pair_word(0, 1).0]), (u32::MAX, u32::MAX));
    assert_eq!(words[&capture_stock::DUMP_SELECTED.0], 1);
    assert_eq!(words[&capture_stock::dump_list_accepted(0, 0).0], 1);
    let dumped: Vec<u32> = (0..4).map(|word| words[&capture_stock::dump_list_area(0, 0, word).0]).collect();
    let top = 10 + 21 * 7 + 2;
    assert_eq!(dumped, [107, top + 9, 16, 16], "corner moved by the extent; width 20 and height 16 rounded to steps 8 and 4");
}

#[test]
fn the_band_moves_towards_an_area_within_the_shift_limit() {
    for (row, shift) in [(2, 4), (13, -4)] {
        let mut world = World::new();
        world.lists[0].push(area_in_row(row));
        requested(&mut world);
        let (request, _) = world.measured();
        assert_eq!((request.area_count, request.shift), (1, shift), "area in row {row}");
    }
}

#[test]
fn an_area_out_of_reach_drops_the_measurement_when_one_is_required() {
    let mut world = World::new();
    world.lists[0].push(area_in_row(0));
    world.setting(MeasurementSetting::ShiftLimit, 1);
    world.setting(MeasurementSetting::RequiresArea, 1);
    requested(&mut world);
    let (request, areas) = world.measured();
    assert_eq!((request.area_count, request.shift), (0, 0));
    assert_eq!(areas, [AreaEntry::EMPTY; 3]);
    let words = world.frame_words();
    assert_eq!(words[&capture_stock::dump_list_count(0).0], 1, "the dump still shows the list it searched");
    assert_eq!(words[&capture_stock::dump_list_pair_word(0, 0).0], 0);
    assert_eq!(words[&capture_stock::DUMP_SELECTED.0], 0);
    assert!(!words.contains_key(&capture_stock::dump_list_accepted(0, 0).0));
}

#[test]
fn an_area_out_of_reach_moves_the_band_by_the_limit_when_none_is_required() {
    let mut world = World::new();
    world.lists[0].push(area_in_row(0));
    world.setting(MeasurementSetting::ShiftLimit, 1);
    requested(&mut world);
    let (request, _) = world.measured();
    assert_eq!((request.area_count, request.shift), (0, 0), "row 0 lies outside the band moved one row up, so nothing is measured");

    let mut world = World::new();
    let mut area = AreaEntry::EMPTY;
    area.set_rectangle([100, 96, 30, 20]);
    area.set_centre([110, 104]);
    world.lists[0].push(area);
    world.setting(MeasurementSetting::ShiftLimit, 1);
    requested(&mut world);
    let (request, _) = world.measured();
    assert_eq!((request.area_count, request.shift), (1, 1), "row 4 overlaps the band moved one row up");
}

#[test]
fn the_second_list_needs_an_area_in_the_first_or_the_setting() {
    let mut world = World::new();
    world.prefers_second = true;
    world.lists[1].push(area_in_row(7));
    requested(&mut world);
    assert_eq!(world.measured().0.area_count, 0);

    let mut world = World::new();
    world.prefers_second = true;
    world.lists[1].push(area_in_row(7));
    world.setting(MeasurementSetting::SecondListAlone, 1);
    requested(&mut world);
    let (request, areas) = world.measured();
    assert_eq!((request.lists, request.area_count), ([false, true], 1));
    assert_eq!(areas[0], area_in_row(7));
}

#[test]
fn a_measurement_needs_the_first_slot_and_the_setting() {
    for (slot, setting) in [(0, 1), (1, 0)] {
        let mut world = World::new();
        world.lists[0].push(area_in_row(7));
        world.slot_fields.insert(SlotField::MeasuresAreas as u8, slot);
        world.setting(MeasurementSetting::Measure, setting);
        requested(&mut world);
        assert_eq!(world.measured().0.area_count, 0);
    }
}

#[test]
fn a_clipped_area_keeps_only_its_part_inside_the_band() {
    let mut world = World::new();
    let mut area = AreaEntry::EMPTY;
    // Rows 6..=9 of the band span 136..=219; this area spans 120..=160 around row 6.
    area.set_rectangle([100, 120, 40, 20]);
    area.set_centre([110, 140]);
    area.set_scale(80);
    world.lists[0].push(area);
    requested(&mut world);
    let (_, areas) = world.measured();
    assert_eq!(areas[0].rectangle(), [100, 136, 24, 20]);
    assert_eq!(areas[0].centre(), [110, 148]);
}

#[test]
fn measurements_alternate_statistics_stores_and_count_the_first_pass() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Image5c, &[0, 0, 0, 0, 3, 0, 0, 0]);
    assert!(job.query(SecondaryQuery::Accepted, &mut world));
    assert!(job.query(SecondaryQuery::FirstPass, &mut world));
    deliver(&mut job, Completion::Image5c, &[0, 0, 0, 0, 4, 0, 0, 0]);
    world.statistics_accepted = false;
    assert!(!job.query(SecondaryQuery::Accepted, &mut world));
    assert!(!job.query(SecondaryQuery::FirstPass, &mut world));
    assert_eq!(
        world.calls.iter().filter(|call| matches!(call, Call::Statistics(..))).cloned().collect::<Vec<_>>(),
        [Call::Statistics(3, StatisticsStore::First), Call::Statistics(4, StatisticsStore::Later)],
    );
    job.effect(SecondaryEffect::PrepareFirst, &parameters(), &mut world);
    let measurement = job.measurement();
    assert_eq!((measurement.second, measurement.first), (3, -3), "the key comes from the first pass's processing");
}

#[test]
fn an_alternative_pass_needs_the_diagnostics_and_a_style_of_four_to_seven() {
    for (diagnostics, style, alternative) in [(1, 4, true), (1, 7, true), (1, 3, false), (1, 8, false), (0, 5, false)] {
        let mut world = World::new();
        world.adjustments.insert(CaptureAdjustment::Diagnostics as u8, diagnostics);
        world.adjustments.insert(CaptureAdjustment::SecondaryStyle as u8, style);
        assert_eq!(begun(&mut world).query(SecondaryQuery::Alternative, &mut world), alternative, "{diagnostics} {style}");
    }
}

#[test]
fn an_alternative_pass_evaluates_with_tenfold_defaults_and_enables_the_job() {
    let mut world = World::new();
    world.setting(MeasurementSetting::MeasuredDefault, 7);
    world.setting(MeasurementSetting::TransferDefault, 3);
    world.setting(MeasurementSetting::PrimaryLimitDefault, 12);
    let mut job = begun(&mut world);
    job.effect(SecondaryEffect::PrepareNext, &parameters(), &mut world);
    assert!(job.enabled());
    let evaluation = job.evaluation();
    assert_eq!((evaluation.measured(), evaluation.transfer(), evaluation.primary_limit(), evaluation.primary()), (70, 30, 120, 0));
    assert!(job.query(SecondaryQuery::TransferPending, &mut world));
    job.effect(SecondaryEffect::RequestTransfer, &parameters(), &mut world);
    assert!(world.calls.contains(&Call::Transfer(TransferRequest { transfer: 30 }, Completion::Image5d)));
}

#[test]
fn the_calculation_carries_the_geometry_and_the_scaled_share() {
    let mut world = World::new();
    world.lists[0].push(area_in_row(7));
    world.words.insert(MeasurementWord::FirstList as u8, 0x99);
    world.crops.insert((8, CAPTURE_OFFSET, false), CropGeometry { frame: [0, 300], window: [0, 0, 0, 600], ..CropGeometry::default() });
    let mut job = requested(&mut world);
    deliver(&mut job, Completion::Image5c, &[0, 0, 0, 0, 2, 0, 0, 0]);
    assert!(job.query(SecondaryQuery::Accepted, &mut world));
    job.effect(SecondaryEffect::RequestCalculation, &parameters(), &mut world);
    let (frames, geometry, completions) = world.calls.iter().find_map(|call| match call { Call::Evaluate(f, g, c) => Some((*f, *g, *c)), _ => None }).unwrap();
    assert_eq!(frames, [FRAME.0, 0x77]);
    assert_eq!(completions, [Completion::Calculation94, Completion::Calculation95]);
    assert_eq!((geometry.active, geometry.list, geometry.area_count, geometry.list_value), (true, Some(EvaluatedList::First), 1, 0x99));
    assert_eq!(geometry.scale, 37, "50 percent of the window times 75 percent of the reference");
    assert_eq!((geometry.span, geometry.within_limit, geometry.accepted), (2, true, [true, false, false]));
}

#[test]
fn an_evaluation_enables_the_job_and_places_the_evaluated_area_in_the_record() {
    let mut world = World::new();
    world.lists[0].push(area_in_row(7));
    world.fields.insert(RecordField::PlacesItem as u8, 1);
    world.slot_fields.insert(SlotField::ProfileSelector as u8, 5);
    world.slot_fields.insert(SlotField::Distance as u8, 20);
    world.crops.insert((8, 0, true), CropGeometry { frame: [400, 600], window: [0; 4], ..CropGeometry::default() });
    world.crops.insert((5, 0, false), CropGeometry { frame: [0; 2], window: [10, 20, 200, 300], ..CropGeometry::default() });
    world.crops.insert((5, CAPTURE_OFFSET, false), CropGeometry { frame: [0; 2], window: [13, 24, 0, 0], ..CropGeometry::default() });
    let mut job = begun(&mut world);
    let payload = evaluation_payload(|payload| {
        put_word(payload, 0, 5);
        put_word(payload, 0x38, 1);
        put_word(payload, 0x3c, 1);
        payload[0x40] = 1;
        put_word(payload, 0x50, 1);
        payload[0x4e..0x50].copy_from_slice(&(-300i16).to_le_bytes());
    });
    deliver(&mut job, Completion::Calculation94, &payload);
    job.effect(SecondaryEffect::AbsorbCalculation, &parameters(), &mut world);
    assert!(job.enabled());
    assert_eq!(world.slot_writes(SlotField::State), [2]);
    assert_eq!(world.slot_writes(SlotField::ItemState), [1]);
    let item = world.calls.iter().find_map(|call| match call { Call::StoreItem(item) => Some(*item), _ => None }).unwrap();
    // Halved into the window (300/600, 200/400), then moved back by (3, 4).
    let top = 10 + 21 * 7 + 2;
    assert_eq!(item.rectangle(), [50 - 3, top / 2 - 4, 8, 10]);
    assert_eq!(item.centre(), [55 - 3, top / 2 + 4 - 4]);
    assert!(world.calls.contains(&Call::SlotExtent([1, 2, 3, 4])));
    assert!(world.calls.contains(&Call::SlotCells([41, 42, 60, 120])));
    assert_eq!(world.slot_writes(SlotField::SecondLimit), [(-300i32) as u32]);
}

#[test]
fn an_evaluation_without_placement_clears_the_first_item() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Calculation94, &evaluation_payload(|_| {}));
    job.effect(SecondaryEffect::AbsorbCalculation, &parameters(), &mut world);
    assert!(!job.enabled());
    assert_eq!(world.slot_writes(SlotField::State), [1]);
    assert_eq!(world.slot_writes(SlotField::ItemState), [0]);
    assert!(world.calls.contains(&Call::StoreItem(AreaEntry::EMPTY)));
    assert!(world.slot_writes(SlotField::SecondLimit).is_empty());
}

#[test]
fn the_refinement_updates_the_evaluation_and_the_first_slot() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let mut payload = [0u8; 84];
    payload[0xc..0xe].copy_from_slice(&(-7i16).to_le_bytes());
    payload[0x16..0x18].copy_from_slice(&21i16.to_le_bytes());
    payload[0x20..0x38].copy_from_slice(&[6; 24]);
    put_word(&mut payload, 0x44, 0xabc);
    deliver(&mut job, Completion::Calculation95, &payload);
    job.effect(SecondaryEffect::AbsorbSecondCalculation, &parameters(), &mut world);
    let measurement = job.measurement();
    assert_eq!((measurement.fifth, measurement.block), (21, [6; 24]));
    assert_eq!(world.slot_writes(SlotField::EvaluationWord), [0xabc]);
    assert_eq!(world.slot_writes(SlotField::EvaluationOffset), [(-7i32) as u32]);
    assert_eq!(world.slot_writes(SlotField::State), [1]);
}

#[test]
fn a_reset_drops_the_evaluation_and_marks_the_key_unset() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Calculation94, &evaluation_payload(|payload| put_word(payload, 0, 9)));
    job.effect(SecondaryEffect::AbsorbCalculation, &parameters(), &mut world);
    assert!(job.enabled());
    job.effect(SecondaryEffect::Absorb, &parameters(), &mut world);
    assert!(!job.enabled());
    assert_eq!(job.evaluation().measured(), 0);
    let measurement = job.measurement();
    assert_eq!((measurement.first, measurement.second), (i16::MIN, i16::MIN));
}

#[test]
fn the_statistics_are_copied_into_the_dump_and_the_measured_value_into_the_record() {
    let mut world = World::new();
    for (word, value) in [(MeasurementWord::DumpFirst, 1), (MeasurementWord::DumpSecond, 2), (MeasurementWord::DumpThird, 3)] { world.words.insert(word as u8, value); }
    let mut job = begun(&mut world);
    deliver(&mut job, Completion::Calculation94, &evaluation_payload(|payload| put_word(payload, 0, 44)));
    job.effect(SecondaryEffect::AbsorbCalculation, &parameters(), &mut world);
    job.effect(SecondaryEffect::AbsorbStatistics, &parameters(), &mut world);
    let words = world.frame_words();
    for ((offset, _), value) in capture_stock::MEASUREMENT_WORDS.into_iter().zip([1, 2, 3]) { assert_eq!(words[&offset.0], value); }
    assert_eq!(world.slot_writes(SlotField::MeasuredValue), [44]);
}

#[test]
fn the_calculations_job_reads_the_evaluation_fields() {
    let mut world = World::new();
    let mut job = begun(&mut world);
    let payload = evaluation_payload(|payload| {
        payload[0xc..0xe].copy_from_slice(&120i16.to_le_bytes());
        put_word(payload, 0x10, 1);
        payload[0x14..0x16].copy_from_slice(&3i16.to_le_bytes());
        payload[0x16..0x18].copy_from_slice(&5i16.to_le_bytes());
        payload[0x18..0x1a].copy_from_slice(&6i16.to_le_bytes());
        payload[0x1a..0x1c].copy_from_slice(&4i16.to_le_bytes());
        payload[0x1c] = 2;
        payload[0x40] = 1;
        payload[0x4c..0x4e].copy_from_slice(&7i16.to_le_bytes());
    });
    deliver(&mut job, Completion::Calculation94, &payload);
    job.effect(SecondaryEffect::AbsorbCalculation, &parameters(), &mut world);
    let measurement = job.measurement();
    assert_eq!((measurement.base, measurement.refined, measurement.third, measurement.fourth, measurement.fifth, measurement.sixth), (120, true, 3, 4, 5, 6));
    assert_eq!((measurement.count, measurement.seventh, measurement.marked), (2, 7, true));
}

#[test]
fn the_release_ends_the_capture_pipeline_measurement() {
    let mut world = World::new();
    begun(&mut world).effect(SecondaryEffect::Release, &parameters(), &mut world);
    assert_eq!(world.calls, [Call::Release]);
}
