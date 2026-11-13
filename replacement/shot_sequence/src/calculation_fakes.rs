//! A calculation block, selection grid and record layer for the absorb and record-load tests.

use crate::absorb::*;
use crate::boundary::*;
use crate::capture_boundary::{AreaReference, RecordField, ReferencePair, SlotEntry, SlotHandle};
use crate::ports::Reply;
use crate::record_load::*;
use xm1_saclac_interface::stock::StockCallbackWord;
use crate::preparation::FrameHeader;
use crate::summary::FrameSummary;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaRow, CellSummary, FocusSummary, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Area(u8, AreaStore),
    Detail(u8, AreaDetail),
    Auxiliary(u8, AuxiliaryReference),
    Cell(u32, u32),
    Summary(FocusSummary, u32),
    Metering(MeteringBlock),
    Export(ResultExport, bool),
    ClearChannels(bool),
    GridUpdate { reset: bool, focus: bool },
    Classify(ClassifyStage),
    RetainResult,
    RestoreResult,
    RetainAuxiliary(AuxiliaryReference, Option<AreaAt>),
    MarkAuxiliaryRetained,
    RetainHalf(u16),
    SetAside,
    RestoreWorking,
    Evaluate(ResultPass, u8),
    AdoptArea(GridPass, u8, Option<TrackedKind>),
    AdoptTables(GridPass, u8),
    AdoptCell(GridPass),
    DrawRateStatus,
    ReleaseRateStatus,
    Snapshot(LensSnapshot),
    ResumeOffset(i16),
    Pair(u8, ReferencePair),
    Starts(AreaStarts),
    ModeFive(ModeFiveBlock),
    ClientSlot { target: u32, slot: u8 },
    Loaded(LoadExport, Option<Frame>),
    RetainWord(u32),
    HookCall(AuxiliaryCall),
    Published(AuxiliaryCall),
    Reply(Reply),
}

pub(crate) struct World {
    pub events: Vec<Event>,
    pub areas: u8,
    pub tracking: bool,
    pub kinds: [u8; 3],
    pub grid_selected: bool,
    pub selection_mode: u32,
    pub auxiliary: AuxiliaryOutcome,
    pub retained_export: u8,
    pub subjects: u8,
    pub rate_status: u8,
    pub map_present: u32,
    pub record_references: [u32; 3],
    pub grid_references: [u32; 3],
    pub split: u32,
    pub lower: u32,
    pub require: bool,
    pub changed: bool,
    pub available: bool,
    pub client_count: u32,
    pub capture_frame: u32,
    pub capture_slots: u32,
    pub subject_source: u32,
    pub lens_reference: u32,
    pub snapshot: LensSnapshot,
    pub profile_offset: i16,
    pub stored_offset: i16,
    pub stored_area: StoredArea,
    pub hook: Option<StockCallbackWord>,
}

impl World {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            areas: 3,
            tracking: false,
            kinds: [UNTRACKED; 3],
            grid_selected: false,
            selection_mode: 0,
            auxiliary: AuxiliaryOutcome::Unchecked,
            retained_export: 0,
            subjects: 0,
            rate_status: 0,
            map_present: 0,
            record_references: [500, 400, 600],
            grid_references: [700, 300, 900],
            split: 1000,
            lower: 77,
            require: false,
            changed: false,
            available: true,
            client_count: 3,
            capture_frame: 0x40,
            capture_slots: 2,
            subject_source: 0,
            lens_reference: 0x51,
            snapshot: crate::stock::calculation::with_result_word(LensSnapshot::default(), 0x6060),
            profile_offset: -12,
            stored_offset: 34,
            stored_area: StoredArea { start: -3, reference: 0x4100, selection_class: 2, plan_half: -7, plan_bit_two: 1, plan_bit_five: 0, halves: [-1, 2, -3, 4, -5] },
            hook: None,
        }
    }

    pub fn area_events(&self) -> Vec<(u8, u32)> {
        self.events.iter().filter_map(|event| match event { Event::Area(index, store) => Some((*index, store.area.reference())), _ => None }).collect()
    }

    pub fn metering(&self) -> Option<(bool, u32)> {
        self.events.iter().rev().find_map(|event| match event {
            Event::Metering(block) => {
                let word = |at: usize| u32::from_le_bytes(block.0[at..at + 4].try_into().unwrap());
                Some((word(0x38) != 0, word(0x3c)))
            }
            _ => None,
        })
    }

    pub fn has(&self, wanted: &Event) -> bool { self.events.iter().any(|event| event == wanted) }

    pub fn position(&self, wanted: &Event) -> usize { self.events.iter().position(|event| event == wanted).unwrap_or_else(|| panic!("{wanted:?} missing")) }

    pub fn pairs(&self) -> Vec<(u8, ReferencePair)> {
        self.events.iter().filter_map(|event| match event { Event::Pair(slot, pair) => Some((*slot, *pair)), _ => None }).collect()
    }
}

pub(crate) fn pass_code(pass: ResultPass) -> u32 {
    match pass {
        ResultPass::Plain => 0,
        ResultPass::Grid(pass) => 1 + u32::from(pass.index()),
    }
}

/// An area whose reference names where it was read; its first halfword is the reference's low half.
pub(crate) fn area_named(reference: u32) -> AreaResult {
    let mut bytes = [0; AreaResult::BYTES];
    bytes[0..2].copy_from_slice(&(reference as u16).to_le_bytes());
    bytes[4..8].copy_from_slice(&reference.to_le_bytes());
    AreaResult::from_bytes(bytes)
}

pub(crate) fn plain_reference(index: u8) -> u32 { 100 + u32::from(index) }

pub(crate) fn tracked_reference(kind: u8, index: u8) -> u32 { 200 + 10 * u32::from(kind) + u32::from(index) }

impl CalculationResults for World {
    fn result_areas(&mut self) -> u8 { self.areas }
    fn tracking(&mut self) -> bool { self.tracking }
    fn area_kind(&mut self, index: AreaIndex) -> u8 { self.kinds[usize::from(index.get())] }
    fn grid_selected(&mut self) -> bool { self.grid_selected }
    fn selection_mode(&mut self) -> u32 { self.selection_mode }
    fn result_area(&mut self, at: AreaAt) -> AreaResult {
        match at {
            AreaAt::Cell { pass: ResultPass::Plain, index, .. } => area_named(plain_reference(index.get())),
            AreaAt::Cell { pass, index, .. } => area_named(self.grid_references[usize::from(index.get())] + pass_code(pass)),
            AreaAt::Tracked { kind, index, .. } => area_named(tracked_reference(kind.get(), index.get())),
        }
    }
    fn marker(&mut self, pass: ResultPass, kind: Option<TrackedKind>, index: AreaIndex) -> u32 {
        1000 * pass_code(pass) + kind.map_or(90, |kind| u32::from(kind.get()) * 10) + u32::from(index.get())
    }
    fn plan_value(&mut self, _pass: ResultPass, _cell: GridCell, _index: AreaIndex) -> u32 { 0x26 }
    fn plan_half(&mut self, _pass: ResultPass, _cell: GridCell, index: AreaIndex) -> i16 { -(i16::from(index.get())) }
    fn row(&mut self, _pass: ResultPass, _cell: GridCell, index: AreaIndex) -> AreaRow {
        let mut row = AreaRow::default();
        row.0[0] = 4;
        row.0[4] = 50 + index.get();
        row
    }
    fn row_word(&mut self, pass: ResultPass, _cell: GridCell) -> u32 { 0x700 + pass_code(pass) }
    fn cell_summary(&mut self, _pass: ResultPass, cell: GridCell) -> CellSummary {
        let mut bytes = [0; CellSummary::BYTES];
        bytes[0] = cell.column;
        bytes[1] = cell.row;
        CellSummary::from_bytes(bytes)
    }
    fn focus_summary(&mut self) -> FocusSummary { FocusSummary([1, 2, 3, 4, 5, 6, 7, 8]) }
    fn retained_half(&mut self) -> u16 { 0x1234 }
    fn auxiliary(&mut self) -> AuxiliaryOutcome { self.auxiliary }
    fn set_auxiliary(&mut self, outcome: AuxiliaryOutcome) { self.auxiliary = outcome; }
    fn set_working_aside(&mut self) { self.events.push(Event::SetAside); }
    fn restore_working(&mut self) { self.events.push(Event::RestoreWorking); }
    fn adopt_grid_area(&mut self, pass: GridPass, _cell: GridCell, index: AreaIndex, kind: Option<TrackedKind>) { self.events.push(Event::AdoptArea(pass, index.get(), kind)); }
    fn adopt_grid_tables(&mut self, pass: GridPass, _cell: GridCell, index: AreaIndex) { self.events.push(Event::AdoptTables(pass, index.get())); }
    fn adopt_grid_cell(&mut self, pass: GridPass, _cell: GridCell) { self.events.push(Event::AdoptCell(pass)); }
}

impl SelectionGrid for World {
    fn cell(&mut self, pass: ResultPass, index: AreaIndex) -> GridCell { GridCell::new(index.get(), pass_code(pass).min(2) as u8).unwrap() }
    fn update_grid(&mut self, _record: RecordHandle, reset: bool, focus: bool, _lens_reference: LensReference) { self.events.push(Event::GridUpdate { reset, focus }); }
    fn classify_subjects(&mut self, stage: ClassifyStage, _reference: &AreaReference, _kind: u32, _source: u32, _area: u32) { self.events.push(Event::Classify(stage)); }
    fn retain_result(&mut self) { self.events.push(Event::RetainResult); }
    fn restore_result(&mut self) { self.events.push(Event::RestoreResult); }
    fn retain_auxiliary(&mut self, auxiliary: &AuxiliaryReference, untracked: Option<AreaAt>) { self.events.push(Event::RetainAuxiliary(*auxiliary, untracked)); }
    fn mark_auxiliary_retained(&mut self) { self.events.push(Event::MarkAuxiliaryRetained); }
    fn requires_eligibility(&mut self) -> bool { self.require }
    fn area_weight(&mut self, index: AreaIndex) -> u32 { 30 + u32::from(index.get()) }
    fn retain_target_half(&mut self, half: u16) { self.events.push(Event::RetainHalf(half)); }
}

impl AbsorbLibrary for World {
    fn subject_source(&mut self, _record: RecordHandle, _refinement: u32) -> u32 { self.subject_source }
    fn grid_mode(&mut self, _record: RecordHandle, mode: u32) -> u32 { if mode == 1 { 0 } else { 4 } }
    fn evaluate_distances(&mut self, pass: ResultPass, _cell: GridCell, index: AreaIndex) { self.events.push(Event::Evaluate(pass, index.get())); }
    fn selection_changed(&mut self, _record: RecordHandle, _source: u32, _reference: u32) -> bool { self.changed }
    fn compare_reference(&mut self, _record: RecordHandle, _source: u32) -> (bool, u32) { (self.available, 0x4444) }
    fn compensated(&mut self, target: u32) -> i16 { target as i16 / 2 }
    fn classify_level(&mut self, value: i16) -> u32 { 0x100 + value as u32 }
    fn split_reference(&mut self) -> u32 { self.split }
    fn lower_reference(&mut self, _lens_reference: LensReference) -> u32 { self.lower }
}

impl AbsorbDiagnostics for World {
    fn adjustment(&mut self, adjustment: AbsorbAdjustment) -> u8 {
        match adjustment {
            AbsorbAdjustment::RetainedExport => self.retained_export,
            AbsorbAdjustment::Subjects => self.subjects,
            AbsorbAdjustment::RateStatus => self.rate_status,
        }
    }
    fn export_result(&mut self, _frame: Option<Frame>, export: ResultExport, enabled: bool) { self.events.push(Event::Export(export, enabled)); }
    fn clear_channel_diagnostics(&mut self, _frame: Option<Frame>, cleared: bool) { self.events.push(Event::ClearChannels(cleared)); }
    fn draw_rate_status(&mut self, _reference: &AreaReference) { self.events.push(Event::DrawRateStatus); }
    fn release_rate_status(&mut self) { self.events.push(Event::ReleaseRateStatus); }
}

impl AbsorbRecords for World {
    fn store_area(&mut self, _record: RecordHandle, index: AreaIndex, store: &AreaStore) { self.events.push(Event::Area(index.get(), *store)); }
    fn store_area_detail(&mut self, _record: RecordHandle, index: AreaIndex, detail: &AreaDetail) { self.events.push(Event::Detail(index.get(), *detail)); }
    fn store_auxiliary(&mut self, _record: RecordHandle, index: AreaIndex, auxiliary: &AuxiliaryReference) { self.events.push(Event::Auxiliary(index.get(), *auxiliary)); }
    fn store_cell(&mut self, _record: RecordHandle, cell: &CellSummary, weight: u32) {
        self.events.push(Event::Cell(u32::from(cell.bytes()[0]) << 8 | u32::from(cell.bytes()[1]), weight));
    }
    fn store_focus_summary(&mut self, _record: RecordHandle, summary: &FocusSummary, level: u32) { self.events.push(Event::Summary(*summary, level)); }
    fn area_lens_reference(&mut self, _record: RecordHandle, index: u8) -> u32 { self.record_references.get(usize::from(index)).copied().unwrap_or(u32::MAX) }
    fn extended_areas(&mut self, _record: RecordHandle) -> bool { true }
    fn selection_source(&mut self, _record: RecordHandle) -> u32 { 0x5050 }
    fn metering(&mut self, _record: RecordHandle) -> MeteringBlock { MeteringBlock([0xee; METERING_BYTES]) }
    fn store_metering(&mut self, _record: RecordHandle, block: &MeteringBlock) { self.events.push(Event::Metering(*block)); }
    fn value(&mut self, _record: RecordHandle, field: RecordField) -> u32 {
        match field {
            RecordField::CaptureFrame => self.capture_frame,
            RecordField::CaptureSlotCount => self.capture_slots,
            RecordField::MapPresent => self.map_present,
            _ => 0,
        }
    }
    fn area_references(&mut self, _record: RecordHandle) -> AreaReference { AreaReference::default() }
}

impl Client for World {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 { self.client_count }
    fn area(&mut self, areas: ClientRef, index: usize) -> u32 { areas.0 + 4 * index as u32 }
    fn write_lens_target(&mut self, _target: ClientRef, _point: LensPoint, _step: u16, _distance: u32) {}
    fn frame(&mut self, slot: ClientRef) -> Option<Frame> { Some(Frame(slot.0)) }
    fn clear_frame_progress(&mut self, _slot: ClientRef) {}
    fn announce_frame(&mut self, _frame: Frame, _header: FrameHeader) {}
    fn write_summary(&mut self, _frame: Frame, _summary: &FrameSummary) {}
}

impl LoadRecords for World {
    fn record_lens_snapshot(&mut self, _record: RecordHandle) -> LensSnapshot { self.snapshot }
    fn store_record_lens_snapshot(&mut self, _record: RecordHandle, snapshot: &LensSnapshot) {
        self.snapshot = *snapshot;
        self.events.push(Event::Snapshot(*snapshot));
    }
    fn lens_reference(&mut self, _record: RecordHandle) -> u32 { self.lens_reference }
    fn resume_offset(&mut self, _record: RecordHandle, source: ResumeSource) -> i16 {
        match source {
            ResumeSource::Profile => self.profile_offset,
            ResumeSource::Stored => self.stored_offset,
        }
    }
    fn store_resume_offset(&mut self, _record: RecordHandle, offset: i16) { self.events.push(Event::ResumeOffset(offset)); }
    fn store_reference_pair(&mut self, _record: RecordHandle, slot: PairSlot, pair: ReferencePair) { self.events.push(Event::Pair(slot.get(), pair)); }
    fn store_area_starts(&mut self, _record: RecordHandle, starts: AreaStarts) { self.events.push(Event::Starts(starts)); }
    fn mode_five_profile(&mut self, _record: RecordHandle) -> [i16; 3] { [-1, 2, -3] }
    fn mode_five_block(&mut self, _record: RecordHandle) -> ModeFiveBlock { ModeFiveBlock([0xaa; MODE_FIVE_BLOCK_BYTES]) }
    fn store_mode_five_block(&mut self, _record: RecordHandle, block: &ModeFiveBlock) { self.events.push(Event::ModeFive(*block)); }
    fn stored_area(&mut self, _record: RecordHandle) -> StoredArea { self.stored_area }
    fn record_slot(&mut self, _record: RecordHandle, slot: u8) -> SlotEntry {
        let mut entry = SlotEntry([0; SlotEntry::BYTES]);
        entry.0[0] = slot;
        entry
    }
    fn store_client_slot(&mut self, target: SlotHandle, entry: &SlotEntry) { self.events.push(Event::ClientSlot { target: target.0, slot: entry.0[0] }); }
    fn auxiliary_hook(&mut self, _record: RecordHandle) -> Option<StockCallbackWord> { self.hook }
}

impl LoadEffects for World {
    fn export_loaded(&mut self, frame: Option<Frame>, export: LoadExport) { self.events.push(Event::Loaded(export, frame)); }
    fn retain_target_word(&mut self, word: u32) { self.events.push(Event::RetainWord(word)); }
    fn call_auxiliary_hook(&mut self, call: &AuxiliaryCall) { self.events.push(Event::HookCall(*call)); }
    fn publish_auxiliary_call(&mut self, call: &AuxiliaryCall) { self.events.push(Event::Published(*call)); }
}

impl Replies for World {
    fn reply(&mut self, reply: Reply) { self.events.push(Event::Reply(reply)); }
}

pub(crate) fn record() -> RecordHandle { record_handle_from_a2(0x2004) }

