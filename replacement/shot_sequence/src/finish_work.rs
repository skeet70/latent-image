#![forbid(unsafe_code)]

//! Leaf work of the finish step: the lens point the finish drives to and its corrections, the
//! parameters of the two lens moves, and what a completed relative move leaves in the lens layer
//! and the client's lens bounds.

use crate::boundary::{Client, Frame, LensPoint, LensReference, Records};
use crate::capture_boundary::{LensMoves, MoveProfile};
use crate::ports::{FinishEffect, FinishQuery, LensMove};
use crate::preparation::Prepared;
use crate::stock::finish::with_capture_status;
use xm1_saclac_interface::RecordHandle;

/// Point state meaning none is recorded; also the target before one is chosen.
const NO_STATE: u32 = 128;
const UNDIRECTED: u32 = 2;
const NEW_CALCULATION: u32 = 1 << 1;
/// Mask bits that choose the point's source: the client's point, then the frame's.
const FRAME_POINT: u32 = 1 << 2;
const CLIENT_POINT: u32 = 1 << 3;
const STEP_UNCLAMPED: u32 = 1 << 1;
/// Load-test settings under which the finish keeps the point as it is.
const LOAD_TESTS: [u32; 2] = [99, 104];
const LENS_STOPPED: u32 = 2;
/// The mode selection's answer that keeps the target selection's mode.
const MODE_UNCHANGED: u32 = 4;
const RESUME_MODE: u32 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishTrace { RelativeMove, AbsoluteMove, RelativeDone, AbsoluteDone }

/// Image-4 profile values the finish reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishProfile {
    /// Nonzero keeps the finish off the remembered position and makes relative moves fast.
    Override,
    /// 1 leaves the client's lens bounds alone.
    SelectionClass,
    LensMode,
    LensKind,
    AreaFlag,
    LensControl,
    LiveTarget,
    DefaultTarget,
}

/// Lens-layer readings (image 4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishLensValue {
    Position,
    PrefersRelative,
    Drive,
    Direction,
    Distance,
    Step,
    MotorState,
    FocusState,
    Busy,
    Manual,
    ReferenceLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishConfiguration {
    /// Zero drops the snapshot point's reference, state and kind.
    KeepPoint,
    PositionUnit,
    PositionScale,
    DriveControl,
    DirectionClamp,
}

/// The positions the bound record keeps for its focus areas; entries `0..=last` are in use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaPositions {
    pub last: u8,
    pub positions: [i16; 4],
    pub bytes: [u8; 36],
}

/// The bound record's pending lens correction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectionRecord {
    pub enabled: u32,
    pub pending: u32,
    pub bytes: [u8; 64],
}

/// Machine 3's choice of the absolute target's source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetSource { pub capture: bool, pub lens_control: bool, pub live: bool }

/// Arguments of the shared position adjustment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionAdjustment {
    pub position: i16,
    pub reference: LensReference,
    pub state: u32,
    pub control: u32,
    pub first: bool,
    pub second: bool,
    pub kind: u32,
    pub record: RecordHandle,
}

/// Pipeline state the finish reads.
pub trait FinishPipeline {
    fn frame_point(&self) -> LensPoint;
    fn relative_first(&self) -> bool;
    fn pipeline_record(&self) -> Option<RecordHandle>;
}

/// Image-4 records, lens layer and configuration.
pub trait FinishLayers: Records + Client {
    fn profile(&mut self, record: RecordHandle, value: FinishProfile) -> u32;
    fn lens(&mut self, value: FinishLensValue) -> u32;
    fn lens_pair(&mut self) -> (u32, u16);
    fn configuration(&mut self, value: FinishConfiguration) -> u32;
    fn load_test(&mut self) -> u32;
    fn trace(&mut self, trace: FinishTrace);
    fn designated_point(&mut self, record: RecordHandle) -> LensPoint;
    fn area_positions(&mut self, record: RecordHandle) -> AreaPositions;
    fn store_area_positions(&mut self, record: RecordHandle, positions: &AreaPositions);
    fn correction_record(&mut self, record: RecordHandle) -> CorrectionRecord;
    fn store_correction_record(&mut self, record: RecordHandle, correction: &CorrectionRecord);
    fn clamp_step(&mut self, position: i16, kind: u32, direction: u32) -> i16;
    fn adjust(&mut self, position: i16, point: &LensPoint, area_flag: bool) -> i16;
    fn near_bounds(&mut self, position: i16, distance: u32, target: u32, drive: u32, direction: u32, step: u32) -> [u32; 3];
    fn far_bounds(&mut self, position: i16, pair: (u32, u16), target: u32, drive: u32, direction: u32, step: u32) -> [u32; 3];
    fn set_lens_target(&mut self, position: i16, reference: LensReference, drive: u32, kind: u32);
    fn store_area_selection(&mut self, record: RecordHandle, index: u8, selection: u32);
    fn store_area_mode(&mut self, record: RecordHandle, index: u8, mode: u32);
    fn notify_bounds(&mut self, bounds: [u32; 3]);
}

/// Values the finish reads from machine 3, the capture command, the frame and the capture pipeline.
pub trait FinishSources {
    fn target_source(&mut self) -> TargetSource;
    fn capture_target_counts(&mut self) -> (u8, u8);
    fn clamp_count(&mut self, count: u8, limit: u8) -> u8;
    fn capture_target(&mut self, index: usize) -> u32;
    fn frame_drive(&mut self, frame: Frame) -> u32;
    fn store_frame_correction(&mut self, frame: Frame, correction: u32);
    fn capture_status(&mut self) -> u32;
}

/// Calculation block, exposure context and saved-input words.
pub trait FinishCalculation {
    fn calculation_record(&mut self) -> RecordHandle;
    fn calculation_areas(&mut self) -> u8;
    fn calculation_selection(&mut self) -> u32;
    fn selection_mode(&mut self) -> u32;
    fn set_selection_mode(&mut self, mode: u32);
    fn exposure_held(&mut self) -> bool;
    fn set_saved_selection_mode(&mut self, mode: u32);
}

/// Calculation-library functions and the shared helpers the finish calls.
pub trait FinishLibrary {
    fn point_selection(&mut self, point: &LensPoint, lens_reference: LensReference, state: u32) -> u32;
    fn target_selection(&mut self, record: RecordHandle, lens_reference: LensReference, position: i16, control: u32, kind: u32, lens_control: u32, frame: u32) -> u32;
    fn mode_selection(&mut self, record: RecordHandle, mode: u32) -> u32;
    fn correction(&mut self, point: &LensPoint, drive: u32, frame_drive: u32) -> u32;
    fn adjust_position(&mut self, adjustment: PositionAdjustment) -> i16;
    /// The finish's record load, shared with the lens return.
    fn load_finish_record(&mut self);
}

pub trait FinishBoundary: FinishLayers + FinishSources + FinishCalculation + FinishLibrary {}
impl<T: FinishLayers + FinishSources + FinishCalculation + FinishLibrary> FinishBoundary for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinishWork {
    record: Option<RecordHandle>,
    point: LensPoint,
    target: u32,
    /// Correction applied to the point; the client sees positions without it.
    correction: u16,
    /// Position a mode-0 finish keeps until the lens point moves further than the configured scale.
    remembered: i16,
}

impl Default for FinishWork { fn default() -> Self { Self::new() } }

const fn unset_point(position: i16) -> LensPoint { LensPoint { position, detail: 0, reference: LensReference(0), state: NO_STATE, kind: UNDIRECTED } }

impl FinishWork {
    pub const fn new() -> Self { Self { record: None, point: unset_point(0), target: NO_STATE, correction: 0, remembered: 0 } }

    pub fn point(&self) -> LensPoint { self.point }

    pub fn effect(&mut self, effect: FinishEffect, prepared: &Prepared, pipeline: &impl FinishPipeline, boundary: &mut impl FinishBoundary) {
        match effect {
            FinishEffect::Begin(record) => self.begin(record, boundary),
            FinishEffect::LoadAdjustment => self.load_point(prepared, pipeline, boundary),
            FinishEffect::PrepareRecords => self.prepare_records(prepared, boundary),
            FinishEffect::PrepareLens => self.prepare_lens(prepared, boundary),
            FinishEffect::SelectTarget => self.select_target(boundary),
            FinishEffect::PrepareLiveTarget => self.prepare_live_target(boundary),
            FinishEffect::AbsorbRelative => self.absorb_relative(prepared, boundary),
            FinishEffect::MarkRelativeDone => boundary.trace(FinishTrace::RelativeDone),
            FinishEffect::MarkAbsoluteDone => boundary.trace(FinishTrace::AbsoluteDone),
            FinishEffect::Clear => {}
        }
    }

    pub fn query(&self, query: FinishQuery, pipeline: &impl FinishPipeline, boundary: &mut impl FinishBoundary) -> bool {
        match query {
            FinishQuery::RelativeFirst => pipeline.relative_first() || boundary.lens(FinishLensValue::PrefersRelative) != 0,
        }
    }

    /// Starts `FinishRelative` or `FinishAbsolute`; other moves are not the finish's.
    pub fn lens_move(&self, lens_move: LensMove, prepared: &Prepared, pipeline: &impl FinishPipeline, boundary: &mut (impl FinishBoundary + LensMoves)) {
        match lens_move {
            LensMove::FinishRelative => {
                boundary.trace(FinishTrace::RelativeMove);
                let lens = boundary.lens(FinishLensValue::Position) as i16;
                let fast = pipeline.pipeline_record().is_some_and(|record| boundary.profile(record, FinishProfile::Override) != 0);
                let speed = if fast { 7 } else { 6 };
                boundary.relative(self.point.position.wrapping_sub(lens), self.point.kind, MoveProfile::Finish { speed }, lens_move.completion());
            }
            LensMove::FinishAbsolute => {
                boundary.trace(FinishTrace::AbsoluteMove);
                let speed = if prepared.pipeline_gate > 1 { 2 } else { 3 };
                boundary.absolute(self.target, MoveProfile::Finish { speed }, lens_move.completion());
            }
            _ => {}
        }
    }

    /// Binds `record` and records the capture pipeline's status in the lens snapshot.
    fn begin(&mut self, record: RecordHandle, boundary: &mut impl FinishBoundary) {
        let position = boundary.lens(FinishLensValue::Position) as i16;
        *self = Self { record: Some(record), point: unset_point(position), target: NO_STATE, correction: 0, remembered: self.remembered };
        let status = boundary.capture_status();
        let snapshot = boundary.lens_snapshot();
        boundary.store_lens_snapshot(&with_capture_status(snapshot, status == 1));
    }

    fn load_point(&mut self, prepared: &Prepared, pipeline: &impl FinishPipeline, boundary: &mut impl FinishBoundary) {
        let (Some(record), Some(parameters)) = (self.record, prepared.parameters) else { return };
        if parameters.mode.0 == 0 && boundary.profile(record, FinishProfile::Override) == 0 {
            self.point = boundary.lens_snapshot().current;
            if boundary.configuration(FinishConfiguration::KeepPoint) == 0 {
                self.point = LensPoint { reference: LensReference(0), state: NO_STATE, kind: UNDIRECTED, ..self.point };
            }
            let unit = boundary.configuration(FinishConfiguration::PositionUnit);
            let scale = boundary.configuration(FinishConfiguration::PositionScale);
            if i32::from(self.point.position).abs_diff(i32::from(self.remembered)) > unit.wrapping_mul(scale) {
                self.remembered = self.point.position;
            }
            self.point.position = self.remembered;
        } else if parameters.mask.0 & CLIENT_POINT == 0 {
            self.point = LensPoint { state: NO_STATE, ..boundary.lens_snapshot().current };
        } else if parameters.mask.0 & FRAME_POINT == 0 {
            self.point = boundary.designated_point(record);
        } else {
            self.point = pipeline.frame_point();
        }
    }

    /// Writes the calculation's area selections and modes for the point, then loads the finish
    /// record.
    fn prepare_records(&mut self, prepared: &Prepared, boundary: &mut impl FinishBoundary) {
        self.select_areas(prepared, boundary);
        boundary.load_finish_record();
    }

    fn select_areas(&mut self, prepared: &Prepared, boundary: &mut impl FinishBoundary) {
        let (Some(record), Some(parameters)) = (self.record, prepared.parameters) else { return };
        let new_calculation = parameters.mask.0 & NEW_CALCULATION != 0;
        let calculation = boundary.calculation_record();
        let control = boundary.profile(calculation, FinishProfile::LensControl);
        let state = if self.point.state == NO_STATE { control } else { self.point.state };
        let selection = boundary.point_selection(&self.point, prepared.lens_reference, state);
        if !new_calculation && parameters.mode.0 != 0 { return; }
        let areas = boundary.calculation_areas();
        for index in 0..areas { boundary.store_area_selection(calculation, index, selection); }
        if !new_calculation || parameters.mode.0 == RESUME_MODE { return; }

        let mut position = self.point.position;
        if self.needs_position_adjust(prepared, boundary) {
            position = boundary.adjust_position(PositionAdjustment {
                position,
                reference: prepared.lens_reference,
                state,
                control,
                first: false,
                second: false,
                kind: self.point.kind,
                record,
            });
        }
        let frame = parameters.frame_slot.and_then(|slot| boundary.frame(slot)).map_or(0, |frame| frame.0);
        let target = boundary.target_selection(calculation, prepared.lens_reference, position, control, self.point.kind, prepared.lens_control, frame);
        boundary.set_selection_mode(target);
        let mode = boundary.mode_selection(calculation, parameters.mode.0);
        if mode != MODE_UNCHANGED { boundary.set_selection_mode(mode); }
        if !boundary.exposure_held() {
            let saved = if boundary.calculation_selection() == 0 {
                3
            } else {
                match boundary.selection_mode() {
                    mode @ (1 | 2) => mode,
                    _ => 0,
                }
            };
            boundary.set_saved_selection_mode(saved);
        }
        let mode = boundary.selection_mode();
        for index in 0..areas { boundary.store_area_mode(calculation, index, mode); }
    }

    fn needs_position_adjust(&self, prepared: &Prepared, boundary: &mut impl FinishBoundary) -> bool {
        let (Some(record), Some(parameters)) = (self.record, prepared.parameters) else { return false };
        match parameters.mode.0 {
            2 | 7 => {}
            0 | 5 | 6 => {
                if boundary.profile(record, FinishProfile::LensMode) == 1
                    && boundary.profile(record, FinishProfile::LensKind) == 3
                    && boundary.profile(record, FinishProfile::Override) == 0
                {
                    return false;
                }
            }
            _ => return false,
        }
        if LOAD_TESTS.contains(&boundary.load_test()) || self.point.state == NO_STATE { return false; }
        (self.point.reference.0 as i32) < boundary.lens(FinishLensValue::ReferenceLimit) as i32
    }

    /// Adjusts the point and the record's area positions to the lens, applies a pending lens
    /// correction, clamps the step to a direction and records the held point in the lens snapshot.
    fn prepare_lens(&mut self, prepared: &Prepared, boundary: &mut impl FinishBoundary) {
        let Some(record) = self.record else { return };
        if self.needs_position_adjust(prepared, boundary) {
            let control = boundary.profile(record, FinishProfile::LensControl);
            let area_flag = boundary.profile(record, FinishProfile::AreaFlag) == 1;
            let point = self.point;
            let adjust = |position| PositionAdjustment {
                position,
                reference: point.reference,
                state: point.state,
                control,
                first: false,
                second: area_flag,
                kind: point.kind,
                record,
            };
            self.point.position = boundary.adjust_position(adjust(point.position));
            let mut areas = boundary.area_positions(record);
            for position in areas.positions.iter_mut().take(usize::from(areas.last) + 1) {
                *position = boundary.adjust_position(adjust(*position));
            }
            boundary.store_area_positions(record, &areas);
            self.point.state = control;
            let mut snapshot = boundary.lens_snapshot();
            snapshot.marker = 1;
            boundary.store_lens_snapshot(&snapshot);
        }

        let frame = prepared.parameters.and_then(|parameters| parameters.frame_slot).and_then(|slot| boundary.frame(slot));
        if let Some(frame) = frame { boundary.store_frame_correction(frame, 0); }
        self.correction = 0;
        let mut pending = boundary.correction_record(record);
        if pending.enabled != 0 && pending.pending != 0 {
            let drive = boundary.lens(FinishLensValue::Drive);
            let frame_drive = frame.map_or(0, |frame| boundary.frame_drive(frame));
            let correction = boundary.correction(&self.point, drive, frame_drive);
            let shift = correction as i16;
            self.point.position = self.point.position.wrapping_add(shift);
            let mut areas = boundary.area_positions(record);
            for position in areas.positions.iter_mut().take(usize::from(areas.last) + 1) { *position = position.wrapping_add(shift); }
            boundary.store_area_positions(record, &areas);
            pending.pending = 0;
            boundary.store_correction_record(record, &pending);
            if let Some(frame) = frame { boundary.store_frame_correction(frame, correction); }
            self.correction = correction as u16;
        }

        if !LOAD_TESTS.contains(&boundary.load_test())
            && boundary.configuration(FinishConfiguration::DriveControl) & STEP_UNCLAMPED == 0
            && self.point.kind != UNDIRECTED
            && boundary.configuration(FinishConfiguration::DirectionClamp) == 1
        {
            let lens = boundary.lens(FinishLensValue::Position) as i16;
            let direction = match self.point.position.cmp(&lens) {
                core::cmp::Ordering::Greater => 0,
                core::cmp::Ordering::Less => 1,
                core::cmp::Ordering::Equal => boundary.lens(FinishLensValue::Direction),
            };
            self.point.position = boundary.clamp_step(self.point.position, self.point.kind, direction);
            self.point.kind = direction;
        }

        let mut snapshot = boundary.lens_snapshot();
        let held = self.client_position(snapshot.marker, boundary);
        snapshot.held.position = held;
        snapshot.held.reference = self.point.reference;
        snapshot.held.state = 0;
        snapshot.held.kind = self.point.kind;
        boundary.store_lens_snapshot(&snapshot);
    }

    /// The point's position without the correction, adjusted when the snapshot is marked.
    fn client_position(&self, marker: u32, boundary: &mut impl FinishBoundary) -> i16 {
        let position = self.point.position.wrapping_sub(self.correction as i16);
        if marker != 1 { return position; }
        let Some(record) = self.record else { return position };
        let area_flag = boundary.profile(record, FinishProfile::AreaFlag) == 1;
        boundary.adjust(position, &self.point, area_flag)
    }

    fn select_target(&mut self, boundary: &mut impl FinishBoundary) {
        let Some(record) = self.record else { return };
        let source = boundary.target_source();
        self.target = if source.capture {
            let (count, limit) = boundary.capture_target_counts();
            let index = usize::from(boundary.clamp_count(count, limit)).saturating_sub(1);
            boundary.capture_target(index)
        } else if source.lens_control {
            boundary.profile(record, FinishProfile::LensControl)
        } else if source.live {
            boundary.profile(record, FinishProfile::LiveTarget)
        } else {
            boundary.profile(record, FinishProfile::DefaultTarget)
        };
    }

    /// The live finish drives only the absolute move, to the live target from the current position.
    fn prepare_live_target(&mut self, boundary: &mut impl FinishBoundary) {
        let Some(record) = self.record else { return };
        self.point = unset_point(boundary.lens(FinishLensValue::Position) as i16);
        self.target = boundary.profile(record, FinishProfile::LiveTarget);
    }

    /// What a completed relative move leaves: the lens layer's target in mode 1 and the client's
    /// lens bounds in modes 0, 5 and 6.
    fn absorb_relative(&mut self, prepared: &Prepared, boundary: &mut impl FinishBoundary) {
        let (Some(record), Some(parameters)) = (self.record, prepared.parameters) else { return };
        match parameters.mode.0 {
            1 => {
                if boundary.lens(FinishLensValue::Busy) != 0 || lens_stopped(boundary) { return; }
                let current = boundary.lens_snapshot().current;
                let area_flag = boundary.profile(record, FinishProfile::AreaFlag) == 1;
                let position = boundary.adjust_position(PositionAdjustment {
                    position: self.point.position,
                    reference: self.point.reference,
                    state: self.point.state,
                    control: current.state,
                    first: area_flag,
                    second: false,
                    kind: self.point.kind,
                    record,
                });
                self.remembered = position;
                let drive = boundary.lens(FinishLensValue::Drive);
                boundary.set_lens_target(position, self.point.reference, drive, self.point.kind);
            }
            0 | 5 | 6 => {
                if boundary.profile(record, FinishProfile::SelectionClass) == 1 { return; }
                let fixed = boundary.profile(record, FinishProfile::Override) != 0;
                if fixed && boundary.target_source().live { return; }
                let marker = boundary.lens_snapshot().marker;
                let position = if fixed { self.client_position(marker, boundary) } else { self.remembered };
                let bounds = if lens_stopped(boundary) {
                    [u32::MAX; 3]
                } else if boundary.lens(FinishLensValue::Manual) == 0 && !fixed {
                    let drive = boundary.lens(FinishLensValue::Drive);
                    let direction = boundary.lens(FinishLensValue::Direction);
                    let distance = boundary.lens(FinishLensValue::Distance);
                    let step = boundary.lens(FinishLensValue::Step);
                    boundary.near_bounds(position, distance, self.target, drive, direction, step)
                } else {
                    let drive = boundary.lens(FinishLensValue::Drive);
                    let direction = boundary.lens(FinishLensValue::Direction);
                    let pair = boundary.lens_pair();
                    let step = boundary.lens(FinishLensValue::Step);
                    boundary.far_bounds(position, pair, self.target, drive, direction, step)
                };
                boundary.notify_bounds(bounds);
            }
            _ => {}
        }
    }
}

fn lens_stopped(boundary: &mut impl FinishBoundary) -> bool {
    boundary.lens(FinishLensValue::MotorState) == LENS_STOPPED || boundary.lens(FinishLensValue::FocusState) == LENS_STOPPED
}
