#![forbid(unsafe_code)]

//! Leaf work of the capture's secondary job: the measurement of the record's areas inside a band
//! of the frame (imgdc 0x3f46), its statistics, the evaluation (calc 0x3c0d) and what the
//! evaluation leaves in the record, and the transfer (imgdc 0x3f47). The job's state is read by
//! the capture controller, the calculations job and the primary job.

use crate::boundary::Frame;
use crate::calculations_work::{Processing, SecondaryMeasurement};
use crate::capture_boundary::{CaptureAdjustment, CaptureClient, CaptureParameters, CapturePlatform, CaptureRecords, RecordField, SlotField};
use crate::capture_stock;
use crate::finish_work::FinishLensValue;
use crate::ports::{SecondaryEffect, SecondaryQuery};
use crate::secondary_boundary::{
    AreaList, AreaLists, CropRequest, GeometryRequest, MeasurementCalc, MeasurementLens, MeasurementPipeline, MeasurementProfiles, MeasurementSetting,
    MeasurementSettings, MeasurementWord, ProfileGeometry, ScalePair, StatisticsStore,
};
use xm1_saclac_calc::geometry::{self, ClipSettings, Grid, Placement, Point, Rectangle, Region};
use xm1_saclac_interface::area_measurement::{
    AreaEntry, CaptureSelection, Evaluation, EvaluatedList, MeasuredAreas, MeasurementReady, MeasurementRequest, Refinement, TransferRequest,
};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::stock::area_measurement as payloads;
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

pub trait SecondaryBoundary:
    CaptureRecords
    + CaptureClient
    + CapturePlatform
    + AreaLists
    + MeasurementCalc
    + MeasurementProfiles
    + MeasurementLens
    + MeasurementPipeline
    + MeasurementSettings
    + ClipSettings
{
}
impl<T> SecondaryBoundary for T where
    T: CaptureRecords
        + CaptureClient
        + CapturePlatform
        + AreaLists
        + MeasurementCalc
        + MeasurementProfiles
        + MeasurementLens
        + MeasurementPipeline
        + MeasurementSettings
        + ClipSettings
{
}

/// The profile-geometry selector of the capture frame.
const CAPTURE_SELECTOR: u32 = 8;
/// Rows and columns of the measurement grid.
const GRID_CELLS: u16 = 16;
/// The first row of the grid's lower half.
const MIDDLE_ROW: i16 = 8;
const MAX_SPAN: u16 = 8;
/// The key pair after a reset.
const UNSET_KEY: i16 = i16::MIN;
/// Subject state of the first slot after an evaluation of area mode 1, else `PLAIN_STATE`.
const AREA_STATE: u32 = 2;
const PLAIN_STATE: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delivered { Ready(MeasurementReady), Evaluation(Evaluation), Refinement(Refinement) }

/// The secondary job's state between a capture's job preparation and the next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryJob {
    record: Option<RecordHandle>,
    selection: CaptureSelection,
    measurements: u8,
    delivered: Option<Delivered>,
    /// The first pass's processing, then the later passes'.
    processing: [Processing; 2],
    geometry: AreaGeometry,
}

impl Default for SecondaryJob {
    fn default() -> Self { Self::new() }
}

/// What the secondary job leaves for the capture controller, the primary job and the calculations
/// job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryResult {
    enabled: bool,
    key: [i16; 2],
    evaluation: Evaluation,
}

impl Default for SecondaryResult {
    fn default() -> Self { Self::new() }
}

impl SecondaryResult {
    pub const fn new() -> Self { Self { enabled: false, key: [0; 2], evaluation: Evaluation([0; Evaluation::BYTES]) } }

    /// Set by a nonzero evaluation and an alternative pass; the capture controller sets and reads
    /// it when it prepares a job.
    pub fn enabled(&self) -> bool { self.enabled }

    pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    /// Drops the evaluation and the key pair.
    pub fn reset(&mut self) {
        self.enabled = false;
        self.evaluation = Evaluation::default();
        self.key = [UNSET_KEY; 2];
    }

    pub fn evaluation(&self) -> &Evaluation { &self.evaluation }

    /// What the second calc job of the calculations job takes.
    pub fn measurement(&self) -> SecondaryMeasurement {
        let evaluation = &self.evaluation;
        SecondaryMeasurement {
            first: self.key[1],
            second: self.key[0],
            base: evaluation.base() as u16,
            refined: evaluation.refined(),
            third: evaluation.third(),
            fourth: evaluation.fourth(),
            fifth: evaluation.fifth(),
            sixth: evaluation.sixth(),
            count: evaluation.count(),
            block: evaluation.block(),
            seventh: evaluation.seventh(),
            marked: evaluation.list_position() != 0,
        }
    }
}

/// The band of grid rows the measurement keeps, as frame coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Band { minimum: Point, maximum: Point }

impl SecondaryJob {
    pub fn new() -> Self {
        Self {
            record: None,
            selection: CaptureSelection::default(),
            measurements: 0,
            delivered: None,
            processing: [Processing::default(); 2],
            geometry: AreaGeometry::default(),
        }
    }

    pub fn effect(&mut self, effect: SecondaryEffect, parameters: &CaptureParameters, result: &mut SecondaryResult, boundary: &mut impl SecondaryBoundary) {
        match effect {
            SecondaryEffect::Begin(record) => self.record = Some(record),
            SecondaryEffect::ResetCount => {
                self.measurements = 0;
                result.reset();
            }
            SecondaryEffect::Configure => {
                if let Some(record) = self.record { self.selection = boundary.capture_selection(record); }
            }
            SecondaryEffect::Request => self.request(parameters, boundary),
            SecondaryEffect::PrepareFirst => result.key = boundary.statistics_key(&self.processing[0]),
            SecondaryEffect::PrepareNext => prepare_next(result, boundary),
            SecondaryEffect::RequestCalculation => self.request_calculation(parameters, boundary),
            SecondaryEffect::Absorb => result.reset(),
            SecondaryEffect::AbsorbStatistics => self.absorb_statistics(parameters, &result.evaluation, boundary),
            SecondaryEffect::AbsorbCalculation => self.absorb_calculation(result, boundary),
            SecondaryEffect::AbsorbSecondCalculation => self.absorb_refinement(&mut result.evaluation, boundary),
            SecondaryEffect::RequestTransfer => boundary.transfer(&TransferRequest { transfer: result.evaluation.transfer() }, Completion::Image5d),
            SecondaryEffect::Release => boundary.release_measurement(),
        }
    }

    pub fn query(&mut self, query: SecondaryQuery, result: &SecondaryResult, boundary: &mut impl SecondaryBoundary) -> bool {
        match query {
            SecondaryQuery::Accepted => self.accept(boundary),
            SecondaryQuery::FirstPass => self.measurements == 1,
            SecondaryQuery::Alternative => {
                boundary.adjustment(CaptureAdjustment::Diagnostics) & 1 != 0 && matches!(boundary.adjustment(CaptureAdjustment::SecondaryStyle), 4..=7)
            }
            SecondaryQuery::TransferPending => result.evaluation.transfer() != 0,
        }
    }

    /// Keeps what the 0x5c, 0x94 and 0x95 completions deliver.
    pub fn receive(&mut self, completed: Completed<'_>) {
        let payload = completed.parameters.bytes();
        let delivered = match completed.completion {
            Completion::Image5c => payloads::measurement_ready(payload).map(Delivered::Ready),
            Completion::Calculation94 => payloads::evaluation(payload).map(Delivered::Evaluation),
            Completion::Calculation95 => payloads::refinement(payload).map(Delivered::Refinement),
            _ => return,
        };
        if delivered.is_some() { self.delivered = delivered; }
    }

    fn accept(&mut self, boundary: &mut impl SecondaryBoundary) -> bool {
        self.measurements = self.measurements.wrapping_add(1);
        let (store, index) = if self.measurements == 1 { (StatisticsStore::First, 0) } else { (StatisticsStore::Later, 1) };
        let source = match self.delivered {
            Some(Delivered::Ready(ready)) => ready.source,
            _ => 0,
        };
        boundary.prepare_statistics(source, store, &mut self.processing[index])
    }
}

fn prepare_next(result: &mut SecondaryResult, boundary: &mut impl SecondaryBoundary) {
    result.enabled = true;
    let tenfold = |boundary: &mut _, setting| MeasurementSettings::measurement_setting(boundary, setting).wrapping_mul(10);
    let measured = tenfold(boundary, MeasurementSetting::MeasuredDefault);
    let transfer = tenfold(boundary, MeasurementSetting::TransferDefault);
    let limit = tenfold(boundary, MeasurementSetting::PrimaryLimitDefault);
    let evaluation = &mut result.evaluation;
    evaluation.set_measured(measured);
    evaluation.set_primary(0);
    evaluation.set_transfer(transfer);
    evaluation.set_base(0);
    evaluation.set_third(0);
    evaluation.set_fifth(0);
    evaluation.set_fourth(0);
    evaluation.set_primary_limit(limit);
    evaluation.set_seventh(0);
}

fn store_subject_state(record: RecordHandle, evaluation: &Evaluation, records: &mut impl CaptureRecords) {
    let state = if evaluation.area_mode() == 1 { AREA_STATE } else { PLAIN_STATE };
    records.set_slot_field(record, 0, SlotField::State, state);
}

/// The evaluated area moved from the full crop into the first slot's crop window.
fn evaluated_item(record: RecordHandle, evaluation: &Evaluation, boundary: &mut impl SecondaryBoundary) -> AreaEntry {
    let words = profile_words(record, boundary);
    let full = boundary.crop_geometry(&words.crop(CAPTURE_SELECTOR, 0, true));
    let selector = boundary.slot_field(record, 0, SlotField::ProfileSelector);
    let window = boundary.crop_geometry(&words.crop(selector, 0, false)).window;
    let offset = boundary.capture_offset(record);
    let shifted = boundary.crop_geometry(&words.crop(selector, offset, false)).window;

    let index = evaluation.list_position().saturating_sub(1);
    let source = match evaluation.list() {
        Some(EvaluatedList::First) => Some(boundary.area(record, AreaList::First, index)),
        Some(EvaluatedList::Second) => Some(boundary.area(record, AreaList::Second, index)),
        None => None,
    };
    let mut item = source.unwrap_or(AreaEntry::EMPTY);
    let from = geometry::Frame { vertical: full.frame[0], horizontal: full.frame[1] };
    let to = geometry::Frame { vertical: window[2], horizontal: window[3] };
    if let Some(placement) = geometry::rescale(source.map(|area| region_of(&area).placement), from, to) { set_placement(&mut item, placement); }
    let moved = [shifted[0].wrapping_sub(window[0]), shifted[1].wrapping_sub(window[1])];
    let mut rectangle = item.rectangle();
    let mut centre = item.centre();
    for axis in 0..2 {
        rectangle[axis] = rectangle[axis].wrapping_sub(moved[axis]);
        centre[axis] = centre[axis].wrapping_sub(moved[axis]);
    }
    item.set_rectangle(rectangle);
    item.set_centre(centre);
    item
}

impl SecondaryJob {
    fn absorb_statistics(&self, parameters: &CaptureParameters, evaluation: &Evaluation, boundary: &mut impl SecondaryBoundary) {
        let frame = boundary.frame(parameters.sources);
        for (offset, word) in capture_stock::MEASUREMENT_WORDS {
            let value = boundary.measurement_word(word);
            boundary.set_frame_word(frame, offset, value);
        }
        if let Some(record) = self.record { boundary.set_slot_field(record, 0, SlotField::MeasuredValue, evaluation.measured()); }
    }

    fn request_calculation(&mut self, parameters: &CaptureParameters, boundary: &mut impl SecondaryBoundary) {
        let Some(record) = self.record else { return };
        let frames = [boundary.frame(parameters.sources).0, boundary.frame_detail(parameters.sources)];
        self.geometry.list_value = match self.geometry.list {
            Some(EvaluatedList::First) => boundary.measurement_word(MeasurementWord::FirstList),
            Some(EvaluatedList::Second) => boundary.measurement_word(MeasurementWord::SecondList),
            None => 0,
        };
        let reference = boundary.reference_value(0);
        let value = boundary.reference_value(capture_stock::processing_reference_index(&self.processing[0]));
        let offset = boundary.capture_offset(record);
        let words = profile_words(record, boundary);
        let crop = boundary.crop_geometry(&words.crop(CAPTURE_SELECTOR, offset, false));
        let lens_share = percent(value, reference);
        let window_share = percent(crop.frame[1], crop.window[3]);
        self.geometry.scale = (u32::from(window_share) * u32::from(lens_share) / 100) as u16;
        boundary.request_evaluation(record, &self.processing[0], frames, &self.geometry, [Completion::Calculation94, Completion::Calculation95]);
    }

    fn absorb_calculation(&self, result: &mut SecondaryResult, boundary: &mut impl SecondaryBoundary) {
        if let Some(Delivered::Evaluation(evaluation)) = self.delivered { result.evaluation = evaluation; }
        if result.evaluation.measured() != 0 { result.enabled = true; }
        let Some(record) = self.record else { return };
        let evaluation = &result.evaluation;
        store_subject_state(record, evaluation, boundary);
        let (item, present) = if boundary.field(record, RecordField::PlacesItem) == 0 {
            (AreaEntry::EMPTY, false)
        } else {
            let list = if boundary.prefers_second_list(record) { AreaList::Second } else { AreaList::First };
            let present = boundary.area_count(record, list) != 0;
            let item = if evaluation.area_mode() == 1 { evaluated_item(record, evaluation, boundary) } else { AreaEntry::EMPTY };
            (item, present)
        };
        boundary.set_slot_field(record, 0, SlotField::ItemState, u32::from(present));
        boundary.store_item(record, 0, &item);
        store_slot_geometry(record, boundary);
        if evaluation.stores_limit() {
            boundary.set_slot_field(record, 0, SlotField::SecondLimit, i32::from(evaluation.second_limit()) as u32);
        }
    }

    fn absorb_refinement(&self, evaluation: &mut Evaluation, boundary: &mut impl SecondaryBoundary) {
        let Some(Delivered::Refinement(refinement)) = self.delivered else { return };
        evaluation.set_fifth(refinement.fifth);
        evaluation.set_block(refinement.block);
        let Some(record) = self.record else { return };
        store_subject_state(record, evaluation, boundary);
        boundary.set_slot_field(record, 0, SlotField::EvaluationWord, refinement.slot_word);
        boundary.set_slot_field(record, 0, SlotField::EvaluationOffset, i32::from(refinement.slot_offset) as u32);
    }

    /// Chooses an area list and a band of grid rows around its areas, clips the areas to the band,
    /// writes the measurement into the frame's dump and asks imgdc to measure.
    fn request(&mut self, parameters: &CaptureParameters, boundary: &mut impl SecondaryBoundary) {
        let Some(record) = self.record else { return };
        let words = profile_words(record, boundary);
        let raw_counts = LISTS.map(|list| boundary.area_count(record, list));
        let list_details = LISTS.map(|list| if raw_counts[list as usize] != 0 { boundary.list_detail(record, list) } else { 0 });
        let mut request = MeasurementRequest {
            subject_source: boundary.field(record, RecordField::SubjectSource),
            subjects_locked: boundary.field(record, RecordField::SubjectsLocked),
            layout: words.layout,
            secondary_mode: boundary.field(record, RecordField::SecondaryMode),
            secondary_option: boundary.field(record, RecordField::SecondaryOption),
            secondary_latch: boundary.field(record, RecordField::SecondaryLatch),
            profile_class: boundary.field(record, RecordField::ProfileClass) as u16,
            capture_offset: boundary.capture_offset(record),
            frame_words: words.frame_words,
            selection: self.selection,
            slot_offset: boundary.slot_field(record, 0, SlotField::Offset) as i16,
            subject_area: boundary.field(record, RecordField::SubjectArea),
            lists: raw_counts.map(|count| count != 0),
            list_details,
            ..MeasurementRequest::default()
        };
        let frame = boundary.frame(parameters.sources);

        let counts = raw_counts.map(|count| (count as u8).min(3));
        let list = if boundary.slot_field(record, 0, SlotField::MeasuresAreas) != 1 {
            None
        } else if boundary.prefers_second_list(record) {
            let second_allowed = counts[0] != 0 || boundary.measurement_setting(MeasurementSetting::SecondListAlone) == 1;
            (second_allowed && counts[1] != 0).then_some(EvaluatedList::Second)
        } else {
            (counts[0] != 0).then_some(EvaluatedList::First)
        };
        let measure = boundary.measurement_setting(MeasurementSetting::Measure) == 1;
        let settings = [MeasurementSetting::SearchAll, MeasurementSetting::CarriedFirst, MeasurementSetting::CarriedSecond]
            .map(|setting| boundary.measurement_setting(setting) as u8);
        self.geometry = AreaGeometry {
            active: list.is_some() && measure,
            list,
            settings,
            count: match list {
                Some(EvaluatedList::First) => counts[0],
                Some(EvaluatedList::Second) => counts[1],
                None => 0,
            },
            span: self.span(boundary),
            ..AreaGeometry::default()
        };

        let measured = if self.geometry.active { self.measure_band(record, counts, &words, boundary) } else { Measured::default() };
        dump(frame, &measured, boundary);

        request.area_count = self.geometry.area_count;
        request.shift = self.geometry.shift;
        let lens_distance = boundary.lens(FinishLensValue::Distance);
        let drive = boundary.lens(FinishLensValue::Drive);
        self.geometry.record_value = boundary.field(record, RecordField::MeasurementOption);
        let position = boundary.lens(FinishLensValue::Position) as i16;
        let direction = boundary.lens(FinishLensValue::Direction);
        let clamped = boundary.clamp_step(position, direction, 1);
        self.geometry.within_limit = boundary.curve_within(lens_distance, self.selection.selection, drive, i32::from(clamped));
        let slot_rate = boundary.slot_field(record, 0, SlotField::Rate);
        self.geometry.slot_rate = slot_rate;
        request.slot_rate = slot_rate;
        boundary.measure_areas(&request, &self.geometry.areas, [Completion::Image5b, Completion::Image5c]);
    }

    /// Rows of the band on each side of the middle: eight less the first of the scale steps, from
    /// the last, that reaches the selection; one when none does.
    fn span(&self, calc: &mut impl MeasurementCalc) -> u16 {
        (0..MAX_SPAN).find(|step| self.selection.first <= calc.scale_step(MAX_SPAN - 1 - step)).map_or(1, |step| MAX_SPAN - step)
    }

    /// Moves the band towards the chosen list's areas, clips both lists' areas to it and keeps
    /// the chosen list's. Clears `active` when no area reaches the band.
    fn measure_band(&mut self, record: RecordHandle, counts: [u8; 2], words: &ProfileWords, boundary: &mut impl SecondaryBoundary) -> Measured {
        let chosen = if self.geometry.list == Some(EvaluatedList::Second) { 1 } else { 0 };
        let counts = if chosen == 1 { counts } else { [counts[0], 0] };
        let lens_distance = boundary.lens(FinishLensValue::Distance);
        let offset = boundary.capture_offset(record);
        let layout = boundary.profile_geometry(&GeometryRequest {
            selector: CAPTURE_SELECTOR,
            distance: lens_distance,
            offset,
            frame_words: words.frame_words,
            layout: words.layout,
            ..GeometryRequest::default()
        });
        let mut measured = Measured { counts, ..Measured::default() };
        for (index, list) in LISTS.into_iter().enumerate() {
            for slot in 0..counts[index] { measured.areas[index][usize::from(slot)] = boundary.area(record, list, slot); }
        }

        let grid = grid_of(&layout);
        let cell_of = |area: &AreaEntry| {
            let [left, top] = area.centre();
            grid.cell_at(Point { left, top })
        };
        let first_row = cell_of(&measured.areas[chosen][0]).row.unwrap_or(0) as i16;
        let candidates = if boundary.measurement_setting(MeasurementSetting::SearchAll) != 0 { usize::from(counts[chosen]) } else { 1 };
        let limit = boundary.measurement_setting(MeasurementSetting::ShiftLimit);
        let span = self.geometry.span as i16;
        let found = measured.areas[chosen][..candidates].iter().enumerate().find_map(|(index, area)| {
            let cell = cell_of(area);
            let (Some(column), Some(row)) = (cell.column, cell.row) else { return None };
            if column >= GRID_CELLS || row >= GRID_CELLS { return None; }
            let row = row as i16;
            let distance = u32::from(rows_outside_band(row, span));
            (distance <= limit).then_some((index as u8 + 1, row, distance))
        });
        let (selected, row, distance) = match found {
            Some(found) => found,
            None if boundary.measurement_setting(MeasurementSetting::RequiresArea) == 0 => (1, first_row, limit),
            None => {
                self.geometry.active = false;
                (0, first_row, 0)
            }
        };
        let rows = (MAX_SPAN as i16).wrapping_sub(span);
        let magnitude = if distance as i32 > i32::from(rows) { rows } else { distance as i16 };
        self.geometry.shift = if row >= MIDDLE_ROW { magnitude.wrapping_neg() } else { magnitude };
        measured.selected = selected;

        if found.is_none() && self.geometry.active {
            let band = band_of(&layout, self.geometry.span, self.geometry.shift);
            if !measured.areas[chosen][..candidates].iter().any(|area| overlaps(area, band)) {
                self.geometry.active = false;
                self.geometry.shift = 0;
                measured.selected = 0;
            }
        }
        if !self.geometry.active { return measured; }

        let band = band_of(&layout, self.geometry.span, self.geometry.shift);
        let cell = geometry::Frame { vertical: layout.cell[0], horizontal: layout.cell[1] };
        let frame = geometry::Frame { vertical: layout.frame[0], horizontal: layout.frame[1] };
        let mut shares = [[0; 3]; 2];
        for list in 0..2 {
            for index in 0..usize::from(counts[list]) {
                let mut region = region_of(&measured.areas[list][index]);
                let accepted = geometry::clip_region(&mut region, band.minimum, band.maximum, cell, boundary);
                if accepted { set_region(&mut measured.areas[list][index], region); }
                measured.accepted[list][index] = accepted;
                // Suspected stock defect, kept until the owner decides: 0xf2ba6c and 0xf2bad4 pass
                // the first area to 0xf4d080 for every index.
                shares[list][index] = rectangle_of(&measured.areas[list][0]).percent_of(frame).unwrap_or(0);
            }
        }
        measured.clipped = true;
        self.geometry.area_count = u32::from(counts[chosen]);
        self.geometry.areas = measured.areas[chosen];
        self.geometry.accepted = measured.accepted[chosen];
        self.geometry.shares = shares[chosen];
        measured
    }
}

const LISTS: [AreaList; 2] = [AreaList::First, AreaList::Second];

/// Both lists' areas as a measurement leaves them for the frame's dump.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Measured {
    areas: [MeasuredAreas; 2],
    counts: [u8; 2],
    /// The areas went through the clip, which marks each one's pair in the dump.
    clipped: bool,
    accepted: [[bool; 3]; 2],
    /// The 1-based area the band was moved to, 0 for none.
    selected: u8,
}

/// The record words every profile-geometry query of the job carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProfileWords {
    frame_words: [u32; 2],
    layout: [u16; 2],
}

impl ProfileWords {
    fn crop(self, selector: u32, offset: u32, full: bool) -> CropRequest {
        CropRequest { selector, offset, frame_words: self.frame_words, layout: self.layout, full }
    }
}

fn profile_words(record: RecordHandle, records: &mut impl CaptureRecords) -> ProfileWords {
    let layout = records.field(record, RecordField::Layout);
    ProfileWords {
        frame_words: [records.field(record, RecordField::FrameFirst), records.field(record, RecordField::FrameSecond)],
        layout: [layout as u16, (layout >> 16) as u16],
    }
}

/// `numerator` hundredfold over `denominator` as signed words, truncated to a halfword.
fn percent(numerator: u32, denominator: u32) -> u16 {
    let denominator = denominator as i32;
    if denominator == 0 { 0 } else { (numerator.wrapping_mul(100) as i32).wrapping_div(denominator) as u16 }
}

/// How many rows `row` lies outside the band of `span` rows on each side of the middle.
fn rows_outside_band(row: i16, span: i16) -> u16 {
    if row > MIDDLE_ROW - 1 - span && row < span + MIDDLE_ROW {
        0
    } else if row < MIDDLE_ROW {
        (MIDDLE_ROW - span - row) as u16
    } else {
        (row - span - (MIDDLE_ROW - 1)) as u16
    }
}

/// The frame's measurement grid, centred in the frame.
fn grid_of(layout: &ProfileGeometry) -> Grid {
    let [margin_top, margin_left] = margins(layout);
    Grid {
        origin: Point { left: margin_left, top: margin_top },
        cells: GRID_CELLS,
        cell_width: layout.cell[1] as u16,
        cell_height: layout.cell[0] as u16,
        gap: geometry::Frame { vertical: u32::from(layout.gap[1]), horizontal: u32::from(layout.gap[0]) },
    }
}

/// The top and left margins around the grid.
fn margins(layout: &ProfileGeometry) -> [u32; 2] {
    let cells = u32::from(GRID_CELLS);
    let margin = |frame: u32, cell: u32, gap: u16| frame.wrapping_sub(cell.wrapping_mul(cells)).wrapping_sub(u32::from(gap).wrapping_mul(cells - 1)) >> 1;
    [margin(layout.frame[0], layout.cell[0], layout.gap[1]), margin(layout.frame[1], layout.cell[1], layout.gap[0])]
}

/// The grid's full width over the rows `8 - span - shift` to `8 + span - shift`.
fn band_of(layout: &ProfileGeometry, span: u16, shift: i16) -> Band {
    let [margin_top, margin_left] = margins(layout);
    let shift = shift as u16;
    let first = u32::from(MAX_SPAN.wrapping_sub(span).wrapping_sub(shift));
    let end = u32::from(span.wrapping_add(MAX_SPAN).wrapping_sub(shift));
    let [cell_height, cell_width] = layout.cell;
    let [gap_horizontal, gap_vertical] = layout.gap.map(u32::from);
    let cells = u32::from(GRID_CELLS);
    Band {
        minimum: Point { left: margin_left, top: gap_vertical.wrapping_add(cell_height).wrapping_mul(first).wrapping_add(margin_top) },
        maximum: Point {
            left: gap_horizontal.wrapping_mul(cells - 1).wrapping_add(cell_width.wrapping_mul(cells)).wrapping_add(margin_left),
            top: cell_height.wrapping_mul(end).wrapping_add(margin_top).wrapping_add(end.wrapping_sub(1).wrapping_mul(gap_vertical)),
        },
    }
}

fn overlaps(area: &AreaEntry, band: Band) -> bool {
    let [left, top, height, width] = area.rectangle();
    top.wrapping_add(height) >= band.minimum.top && top <= band.maximum.top && left.wrapping_add(width) >= band.minimum.left && left <= band.maximum.left
}

fn rectangle_of(area: &AreaEntry) -> Rectangle {
    let [left, top, height, width] = area.rectangle();
    Rectangle { left, top, height, width }
}

fn region_of(area: &AreaEntry) -> Region {
    let [left, top] = area.centre();
    Region { placement: Placement { rectangle: rectangle_of(area), centre: Point { left, top } }, scale: area.scale() }
}

fn set_placement(area: &mut AreaEntry, placement: Placement) {
    let rectangle = placement.rectangle;
    area.set_rectangle([rectangle.left, rectangle.top, rectangle.height, rectangle.width]);
    area.set_centre([placement.centre.left, placement.centre.top]);
}

fn set_region(area: &mut AreaEntry, region: Region) {
    set_placement(area, region.placement);
    area.set_scale(region.scale);
}

/// Writes both lists' measurement into the frame's dump.
fn dump(frame: Frame, measured: &Measured, boundary: &mut (impl CaptureClient + MeasurementProfiles)) {
    for (list, count) in measured.counts.into_iter().enumerate() {
        boundary.set_frame_word(frame, capture_stock::dump_list_count(list), u32::from(count));
    }
    let pair = if measured.clipped { u32::MAX } else { 0 };
    for (list, count) in measured.counts.into_iter().enumerate() {
        for word in 0..2 * count { boundary.set_frame_word(frame, capture_stock::dump_list_pair_word(list, word), pair); }
    }
    boundary.set_frame_word(frame, capture_stock::DUMP_SELECTED, u32::from(measured.selected));
    if measured.selected != 0 {
        for (list, count) in measured.counts.into_iter().enumerate() {
            for index in 0..count {
                let accepted = measured.accepted[list][usize::from(index)];
                boundary.set_frame_word(frame, capture_stock::dump_list_accepted(list, index), u32::from(accepted));
            }
        }
    }
    for (list, count) in measured.counts.into_iter().enumerate() {
        let step = boundary.flagged_scale_pair(CAPTURE_SELECTOR);
        let extent = boundary.profile_extent(CAPTURE_SELECTOR, false);
        for index in 0..count {
            let words = dumped_area(&measured.areas[list][usize::from(index)], step, extent);
            for (word, value) in words.into_iter().enumerate() { boundary.set_frame_word(frame, capture_stock::dump_list_area(list, index, word), value); }
        }
    }
}

/// An area as the dump shows it: its corner moved by the profile extent, its sides rounded down
/// to the scale steps and at least one step.
fn dumped_area(area: &AreaEntry, step: ScalePair, extent: [u32; 4]) -> [u32; 4] {
    let [left, top, height, width] = area.rectangle();
    let rounded = |side: u32, step: u32| {
        let down = side.wrapping_sub(side.checked_rem(step).unwrap_or(0));
        u32::from(down.max(step) as u16)
    };
    [
        u32::from((extent[0] as u16).wrapping_add(left as u16)),
        u32::from((extent[1] as u16).wrapping_add(top as u16)),
        rounded(width, step.second),
        rounded(height, step.first),
    ]
}

/// Stores the first slot's profile extent and grid geometry after an evaluation.
fn store_slot_geometry(record: RecordHandle, boundary: &mut impl SecondaryBoundary) {
    let words = profile_words(record, boundary);
    let limits = [SlotField::SecondLimit, SlotField::Trim].map(|field| boundary.slot_field(record, 0, field) as i16);
    let reference = boundary.slot_field(record, 0, SlotField::Reference);
    let distance = boundary.slot_field(record, 0, SlotField::Distance);
    let offset = boundary.capture_offset(record);
    let request = GeometryRequest { selector: CAPTURE_SELECTOR, limits, reference, distance, offset, frame_words: words.frame_words, layout: words.layout };
    let extent = boundary.profile_extent(CAPTURE_SELECTOR, true);
    let layout = boundary.profile_geometry(&request);
    boundary.store_slot_extent(record, extent);
    let [vertical, horizontal] = layout.cell;
    let [first, second] = layout.factors.map(u32::from);
    boundary.store_slot_cells(record, [layout.extent[0], layout.extent[1], vertical.wrapping_mul(first), horizontal.wrapping_mul(second)]);
}
