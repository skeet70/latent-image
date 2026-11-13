#![forbid(unsafe_code)]

//! The record load: what the absorbed calculation leaves in the bound record, the client's slots
//! and the record's shared block when a half press finishes and when the capture's lens return
//! concludes.

use crate::absorb::{store_areas, store_cell, AbsorbAdjustment, AbsorbBoundary, AreaTarget, AuxiliaryOutcome, ClassifyStage, Form, FIRST_AREA};
use crate::boundary::{ClientRef, Frame, LensSnapshot, Replies};
use crate::capture_boundary::{RecordField, ReferencePair, SlotEntry, SlotHandle};
use crate::ports::Reply;
use crate::preparation::HalfPressParameters;
use crate::stock::record_load as layout;
use xm1_saclac_interface::calculation::{AreaAt, GridCell, GridPass, ResultPass};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{CommandKind, CompletionStatus, RecordHandle};

const MODE_FIVE: u32 = 5;
/// The half-press mode that hands the first area to the record's auxiliary hook.
const AUXILIARY_MODE: u32 = 7;
/// Selection sources whose record takes the client's reference pair as its third.
const CLIENT_REFERENCE_SOURCES: [u32; 4] = [46, 47, 54, 56];
/// The half-press notification for kept areas; the client's slots take the record's areas.
pub const AREAS_KEPT_STATUS: CompletionStatus = CompletionStatus(4);

/// Which step loads the record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadOrigin {
    HalfPress(HalfPressParameters),
    /// The capture's lens return; `pending` while its lens point is still to be reached.
    Return { pending: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordLoad {
    pub origin: LoadOrigin,
    /// The record the calculation block is bound to.
    pub record: RecordHandle,
    pub refinement: u32,
    /// The exposure asked for the half press to be repeated.
    pub repeat: bool,
}

/// One of the record's five reference pairs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairSlot(u8);

impl PairSlot {
    pub const FIRST: Self = Self(0);
    pub const SECOND: Self = Self(1);
    pub const THIRD: Self = Self(2);
    pub const FIFTH: Self = Self(4);
    pub const ALL: [Self; 5] = [Self(0), Self(1), Self(2), Self(3), Self(4)];

    pub const fn get(self) -> u8 { self.0 }
}

/// Where the lens snapshot's resume offset comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeSource {
    /// The offset the record's profile names.
    Profile,
    /// The offset the record last stored.
    Stored,
}

pub const MODE_FIVE_BLOCK_BYTES: usize = 76;

/// The record's shared block a mode-5 half press rewrites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeFiveBlock(pub [u8; MODE_FIVE_BLOCK_BYTES]);

/// What a mode-5 half press writes into its block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModeFiveValues {
    /// Three profile halfwords of the record.
    pub profile: [i16; 3],
    /// The row word of the selected pass's first cell.
    pub row_word: u32,
}

/// The bound record's first area as the area stores left it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredArea {
    /// The area result's first halfword.
    pub start: i16,
    pub reference: u32,
    pub selection_class: u32,
    pub plan_half: i16,
    pub plan_bit_two: u32,
    pub plan_bit_five: u32,
    /// Area result halfwords the hook takes, in its order.
    pub halves: [i16; 5],
}

/// The first halfwords of the record's first area and of the selected pass's first area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaStarts {
    pub stored: i16,
    pub selected: i16,
}

/// The call mode 7 makes into the record's auxiliary hook, which the shot also publishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxiliaryCall {
    pub hook: StockCallbackWord,
    pub area: StoredArea,
    /// The lens snapshot's calculation word.
    pub reference: u32,
}

/// Frame diagnostics the calculation library writes for the loaded record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadExport {
    /// The selected pass's areas.
    Areas,
    /// The selected pass's first cell.
    Cell,
}

/// The image-4 record layer's values the record load reads and writes.
pub trait LoadRecords {
    fn record_lens_snapshot(&mut self, record: RecordHandle) -> LensSnapshot;
    fn store_record_lens_snapshot(&mut self, record: RecordHandle, snapshot: &LensSnapshot);
    fn lens_reference(&mut self, record: RecordHandle) -> u32;
    fn resume_offset(&mut self, record: RecordHandle, source: ResumeSource) -> i16;
    fn store_resume_offset(&mut self, record: RecordHandle, offset: i16);
    fn store_reference_pair(&mut self, record: RecordHandle, slot: PairSlot, pair: ReferencePair);
    fn store_area_starts(&mut self, record: RecordHandle, starts: AreaStarts);
    fn mode_five_profile(&mut self, record: RecordHandle) -> [i16; 3];
    fn mode_five_block(&mut self, record: RecordHandle) -> ModeFiveBlock;
    fn store_mode_five_block(&mut self, record: RecordHandle, block: &ModeFiveBlock);
    fn stored_area(&mut self, record: RecordHandle) -> StoredArea;
    fn record_slot(&mut self, record: RecordHandle, slot: u8) -> SlotEntry;
    fn store_client_slot(&mut self, target: SlotHandle, entry: &SlotEntry);
    fn auxiliary_hook(&mut self, record: RecordHandle) -> Option<StockCallbackWord>;
}

/// What the record load does outside the record.
pub trait LoadEffects {
    fn export_loaded(&mut self, frame: Option<Frame>, export: LoadExport);
    /// The retained target word and its copy in the saved calculation input.
    fn retain_target_word(&mut self, word: u32);
    fn call_auxiliary_hook(&mut self, call: &AuxiliaryCall);
    /// Publishes the hook's call under the interface lock.
    fn publish_auxiliary_call(&mut self, call: &AuxiliaryCall);
}

pub trait LoadBoundary: AbsorbBoundary + LoadRecords + LoadEffects + Replies {}
impl<T: AbsorbBoundary + LoadRecords + LoadEffects + Replies> LoadBoundary for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Plan {
    mode: u32,
    frame: Option<Frame>,
    /// The client keeps its areas: they reach its slots through the kept-areas notification.
    whole: bool,
    returning: bool,
    pending: bool,
    client_reference: ReferencePair,
    slots: Option<ClientSlots>,
}

/// The client's slot list and how many slots it names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientSlots {
    list: ClientRef,
    count: u32,
}

impl Plan {
    fn of(load: &RecordLoad, boundary: &mut impl LoadBoundary) -> Self {
        match load.origin {
            LoadOrigin::Return { pending } => {
                let frame = boundary.value(load.record, RecordField::CaptureFrame);
                Plan {
                    mode: 0,
                    frame: (frame != 0).then_some(Frame(frame)),
                    whole: true,
                    returning: true,
                    pending,
                    client_reference: ReferencePair::default(),
                    slots: None,
                }
            }
            LoadOrigin::HalfPress(parameters) => {
                let frame = parameters.frame_slot.and_then(|slot| boundary.frame(slot));
                let count = parameters.area_count.map_or(0, |count| boundary.area_count(count));
                Plan {
                    mode: parameters.mode.0,
                    frame,
                    whole: !parameters.mask.publishes_areas(),
                    returning: false,
                    pending: false,
                    client_reference: parameters.client_reference,
                    slots: parameters.areas.map(|list| ClientSlots { list, count }),
                }
            }
        }
    }

    fn target(&self, record: RecordHandle) -> AreaTarget { AreaTarget { record, mode: self.mode, skip_first: self.returning && self.pending } }
}

/// Loads the calculation block's result into the bound record and notifies the client.
pub fn load(load: &RecordLoad, boundary: &mut impl LoadBoundary) {
    let record = load.record;
    let plan = Plan::of(load, boundary);
    let source = boundary.subject_source(record, load.refinement);
    let grid = if boundary.grid_selected() { Some(GridPass::new(boundary.selection_mode())) } else { None };
    let selected = match grid {
        Some(Some(pass)) => ResultPass::Grid(pass),
        _ => ResultPass::Plain,
    };
    if !plan.returning && plan.mode == MODE_FIVE { rewrite_mode_five(record, selected, boundary); }

    let mut areas_loaded = false;
    if let Some(pass) = grid {
        if boundary.tracking() && boundary.adjustment(AbsorbAdjustment::Subjects) & 3 != 0 {
            let reference = boundary.area_references(record);
            let kind = boundary.value(record, RecordField::SubjectKind);
            let area = boundary.value(record, RecordField::SubjectArea);
            boundary.classify_subjects(ClassifyStage::RecordLoad, &reference, kind, source, area);
        }
        let form = if plan.whole || matches!(plan.mode, 0..=2) {
            Some(Form::Full)
        } else {
            boundary.tracking().then_some(Form::Short)
        };
        if let (Some(form), Some(pass)) = (form, pass) {
            store_areas(form, ResultPass::Grid(pass), plan.target(record), boundary);
            areas_loaded = true;
        }
    }

    let stored = boundary.stored_area(record);
    let first_pair = ReferencePair { reference: boundary.lens_reference(record), detail: stored.reference };
    if plan.whole && plan.mode != AUXILIARY_MODE && !plan.returning {
        store_resume(record, load.repeat, boundary);
        for slot in PairSlot::ALL { boundary.store_reference_pair(record, slot, first_pair); }
        if CLIENT_REFERENCE_SOURCES.contains(&source) { boundary.store_reference_pair(record, PairSlot::THIRD, plan.client_reference); }
        let selected_area = boundary.result_area(AreaAt::Cell { pass: selected, cell: GridCell::default(), index: FIRST_AREA });
        let starts = AreaStarts { stored: stored.start, selected: layout::area_start(&selected_area) };
        boundary.store_area_starts(record, starts);
    }
    if matches!(plan.mode, 0 | 6) && !plan.returning { boundary.store_reference_pair(record, PairSlot::FIRST, first_pair); }

    let mut cell_loaded = false;
    if let (Some(Some(pass)), true) = (grid, plan.whole || matches!(plan.mode, 0..=2)) {
        let cell = boundary.cell(ResultPass::Grid(pass), FIRST_AREA);
        boundary.adopt_grid_cell(pass, cell);
        store_cell(record, cell, boundary);
        cell_loaded = true;
    }
    if areas_loaded { boundary.export_loaded(plan.frame, LoadExport::Areas); }
    if cell_loaded { boundary.export_loaded(plan.frame, LoadExport::Cell); }

    if let (false, true, Some(slots)) = (plan.whole, plan.mode <= 2, plan.slots) {
        for slot in (0..slots.count).map_while(|index| u8::try_from(index).ok()) {
            let entry = boundary.record_slot(record, slot);
            let target = SlotHandle(boundary.area(slots.list, usize::from(slot)));
            boundary.store_client_slot(target, &entry);
        }
    }
    boundary.retain_target_word(stored.reference);

    if plan.mode == AUXILIARY_MODE {
        call_auxiliary(record, stored, boundary);
    } else if plan.whole && !plan.returning {
        boundary.reply(Reply { command: CommandKind::HalfPress, record: Some(record), status: AREAS_KEPT_STATUS });
    }
}

fn rewrite_mode_five(record: RecordHandle, selected: ResultPass, boundary: &mut impl LoadBoundary) {
    let mut block = boundary.mode_five_block(record);
    let profile = boundary.mode_five_profile(record);
    let cell = boundary.cell(selected, FIRST_AREA);
    let row_word = boundary.row_word(selected, cell);
    layout::set_mode_five(&mut block, ModeFiveValues { profile, row_word });
    boundary.store_mode_five_block(record, &block);
}

/// The lens snapshot and the record take the resume offset.
fn store_resume(record: RecordHandle, repeat: bool, boundary: &mut impl LoadBoundary) {
    let snapshot = boundary.record_lens_snapshot(record);
    let offset = boundary.resume_offset(record, if repeat { ResumeSource::Stored } else { ResumeSource::Profile });
    boundary.store_record_lens_snapshot(record, &layout::with_resume_offset(snapshot, offset));
    boundary.store_resume_offset(record, offset);
}

/// Mode 7 points the record's reference pairs at the snapshot's calculation word and hands the
/// first area to the record's auxiliary hook.
fn call_auxiliary(record: RecordHandle, stored: StoredArea, boundary: &mut impl LoadBoundary) {
    let snapshot = boundary.record_lens_snapshot(record);
    let pair = ReferencePair { reference: crate::stock::calculation::result_word(&snapshot), detail: stored.reference };
    let auxiliary = boundary.auxiliary();
    if auxiliary != AuxiliaryOutcome::Replaced { boundary.store_reference_pair(record, PairSlot::FIFTH, pair); }
    if auxiliary == AuxiliaryOutcome::Unchecked { boundary.store_reference_pair(record, PairSlot::THIRD, pair); }
    boundary.store_reference_pair(record, PairSlot::SECOND, pair);
    let Some(hook) = boundary.auxiliary_hook(record) else { return };
    let call = AuxiliaryCall { hook, area: stored, reference: pair.reference };
    boundary.call_auxiliary_hook(&call);
    boundary.publish_auxiliary_call(&call);
}
