use super::{Action, Candidate, Controller, Failure, Phase, Point, Position, Score};

// Policy source: af.cpp at raspberrypi/libcamera 957f0f4bd06a163466ff1f3adda66f64fb66cd64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibcameraMode { Triggered, Continuous }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusStatus { Focused, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelMean(u32);
impl ChannelMean {
    pub fn new(value: u32) -> Option<Self> { (value <= 1_000_000).then_some(Self(value)) }
    pub const fn units(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbMeans([ChannelMean; 3]);
impl RgbMeans {
    pub const fn new(red: ChannelMean, green: ChannelMean, blue: ChannelMean) -> Self {
        Self([red, green, blue])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneStatistics {
    pub(super) contrast: Score,
    pub(super) means: RgbMeans,
}
impl SceneStatistics {
    pub const fn new(contrast: Score, means: RgbMeans) -> Self { Self { contrast, means } }
}

// Host Q8 dioptre representation.
const TICKS_PER_DIOPTRE: u16 = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LensFocus(u16);
#[derive(Clone, Copy, Debug)]
struct LensStep(i16);
#[derive(Clone, Copy, Debug)]
struct Frames(u8);
#[derive(Clone, Copy, Debug)]
struct Ratio { numerator: u32, denominator: u32 }
// RangeDependentParams: focusMin = 0.0, focusMax = 12.0 dioptres.
const FOCUS_MIN: LensFocus = LensFocus(0);
const FOCUS_MAX: LensFocus = LensFocus(12 * TICKS_PER_DIOPTRE);
// SpeedDependentParams: stepCoarse = 1.0, stepFine = 0.25, maxSlew = 2.0 dioptres.
const COARSE: LensStep = LensStep(TICKS_PER_DIOPTRE as i16);
const FINE: LensStep = LensStep((TICKS_PER_DIOPTRE / 4) as i16);
const MAX_SLEW: LensStep = LensStep((2 * TICKS_PER_DIOPTRE) as i16);
// SpeedDependentParams: contrastRatio = 0.75, retriggerRatio = 0.75.
const CONTRAST_RATIO: Ratio = Ratio { numerator: 3, denominator: 4 };
const RETRIGGER_RATIO: Ratio = Ratio { numerator: 3, denominator: 4 };
// CfgParams: skipFrames = 5. SpeedDependentParams: stepFrames = 4, retriggerDelay = 10.
const STARTUP_FRAMES: Frames = Frames(5);
const STEP_FRAMES: Frames = Frames(4);
const RETRIGGER_DELAY: Frames = Frames(10);
// findPeak: denominator tolerance 1/64 in contrast-dioptre units, converted to Q8.
const MIN_DENOMINATOR: i64 = (TICKS_PER_DIOPTRE / 64) as i64;
// doScan: terminate a fine scan after at least three records.
const FINE_SAMPLES: usize = 3;
// findPeak: at least three records permit interpolation.
const PEAK_POINTS: usize = 3;
// startProgrammedScan: boundary proximity is twice stepCoarse.
const BOUNDARY_STEPS: u16 = 2;
// doAF: add 1.0 before each contrast/RGB scene-change ratio comparison.
const SCENE_OFFSET: u32 = 1;

impl LensFocus {
    fn offset(self, step: LensStep) -> Self {
        Self((i32::from(self.0) + i32::from(step.0)).clamp(i32::from(FOCUS_MIN.0), i32::from(FOCUS_MAX.0)) as u16)
    }
    fn advance(self, target: Self) -> Self {
        let delta = i32::from(target.0) - i32::from(self.0);
        Self((i32::from(self.0) + delta.clamp(-i32::from(MAX_SLEW.0), i32::from(MAX_SLEW.0))) as u16)
    }
    fn from_position(position: Position, low: Position, high: Position) -> Self {
        let span = u32::from(high.0 - low.0);
        Self(((u32::from(position.0 - low.0) * u32::from(FOCUS_MAX.0) + span / 2) / span) as u16)
    }
    fn position(self, low: Position, high: Position) -> Position {
        let scale = u32::from(FOCUS_MAX.0);
        Position(low.0 + ((u32::from(self.0) * u32::from(high.0 - low.0) + scale / 2) / scale) as u16)
    }
}
impl Ratio {
    fn below(self, value: u32, reference: u32) -> bool {
        u64::from(value) * u64::from(self.denominator) < u64::from(reference) * u64::from(self.numerator)
    }
    fn scene_changed(self, current: u32, reference: u32) -> bool {
        self.below(current + SCENE_OFFSET, reference) || self.below(reference + SCENE_OFFSET, current)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan { CoarseFirst, CoarseReturn, Fine, Settle, Monitor(FocusStatus) }
#[derive(Debug)]
pub(super) struct LibcameraState {
    scan: Scan,
    target: LensFocus,
    smooth: LensFocus,
    step: LensStep,
    skip: Frames,
    delay: Frames,
    stable: Frames,
    scan_start: usize,
    best_index: usize,
    maximum: Score,
    minimum: Option<Score>,
    scene_contrast: Score,
    scene_means: RgbMeans,
    focuses: [LensFocus; 64],
}
impl LibcameraState {
    pub(super) const fn new() -> Self {
        Self { scan: Scan::CoarseReturn, target: FOCUS_MIN, smooth: FOCUS_MIN, step: COARSE,
            skip: Frames(0), delay: Frames(0), stable: Frames(0), scan_start: 0, best_index: 0,
            maximum: Score(0), minimum: None, scene_contrast: Score(0),
            scene_means: RgbMeans([ChannelMean(0); 3]), focuses: [FOCUS_MIN; 64] }
    }
}
impl Controller {
    pub(super) fn libcamera_requires_scene(&self) -> bool {
        matches!(self.candidate, Candidate::LibcameraContrast(LibcameraMode::Continuous))
    }
    pub(super) fn libcamera_needs_frame(&self) -> bool {
        self.libcamera.skip.0 > 0 || self.libcamera.delay.0 > 0 || self.libcamera.smooth != self.libcamera.target
    }
    fn libcamera_issue(&mut self) -> Action {
        self.libcamera.smooth = self.libcamera.smooth.advance(self.libcamera.target);
        let position = self.libcamera.smooth.position(self.config.low, self.config.high);
        let action = self.request(position);
        let Action::Observe { request, position } = action else { return action; };
        if self.libcamera_needs_frame() { Action::WaitFrame { request, position } }
        else if let Scan::Monitor(status) = self.libcamera.scan { Action::Monitor { request, position, status } }
        else { action }
    }
    pub(super) fn libcamera_start(&mut self, position: Position) -> Action {
        self.phase = Phase::Libcamera;
        self.libcamera.smooth = LensFocus::from_position(position, self.config.low, self.config.high);
        self.libcamera.skip = STARTUP_FRAMES;
        self.libcamera_programmed_scan()
    }
    fn libcamera_programmed_scan(&mut self) -> Action {
        let current = self.libcamera.smooth;
        let near_low = current.0 <= BOUNDARY_STEPS * COARSE.0 as u16;
        let near_high = current.0 >= FOCUS_MAX.0 - BOUNDARY_STEPS * COARSE.0 as u16;
        let (target, step, scan) = if !self.libcamera_requires_scene() || near_low {
            (FOCUS_MIN, COARSE, Scan::CoarseReturn)
        } else if near_high {
            (FOCUS_MAX, LensStep(-COARSE.0), Scan::CoarseReturn)
        } else { (current, LensStep(-COARSE.0), Scan::CoarseFirst) };
        self.libcamera.target = target;
        self.libcamera.step = step;
        self.libcamera.scan = scan;
        self.libcamera.scan_start = usize::from(self.count);
        self.libcamera.maximum = Score(0);
        self.libcamera.minimum = None;
        self.libcamera.stable = Frames(0);
        self.libcamera.delay = STEP_FRAMES;
        self.libcamera_issue()
    }
    pub(super) fn libcamera_frame(&mut self) -> Action {
        if self.libcamera.skip.0 > 0 { self.libcamera.skip.0 -= 1; }
        else if self.libcamera.smooth == self.libcamera.target && self.libcamera.delay.0 > 0 {
            self.libcamera.delay.0 -= 1;
        }
        self.libcamera_issue()
    }
    fn libcamera_peak(&self) -> LensFocus {
        let state = &self.libcamera;
        let mut index = state.best_index;
        let fallback = state.focuses[index];
        let end = usize::from(self.count);
        if end - state.scan_start < PEAK_POINTS { return fallback; }
        if index == state.scan_start { index += 1; }
        else if index + 1 == end { index -= 1; }
        let center = i64::from(state.focuses[index].0);
        let left = i64::from(state.focuses[index - 1].0) - center;
        let right = i64::from(state.focuses[index + 1].0) - center;
        let rise = i64::from(self.points[index - 1].score.0) - i64::from(self.points[index].score.0);
        let fall = i64::from(self.points[index + 1].score.0) - i64::from(self.points[index].score.0);
        let denominator = 2 * (rise * right - fall * left);
        if denominator.abs() < MIN_DENOMINATOR || denominator * left <= 0 { return fallback; }
        let offset = (rise * right * right - fall * left * left) / denominator;
        LensFocus((center + offset.clamp(left.min(right), left.max(right))) as u16)
    }
    pub(super) fn libcamera_sample(&mut self, point: Point, means: Option<RgbMeans>) -> Action {
        if matches!(self.libcamera.scan, Scan::Monitor(_)) {
            return match means {
                Some(means) => self.libcamera_monitor(point.score, means),
                None => self.terminal(Action::Fail(Failure::InvalidObservation)),
            };
        }
        if self.libcamera.scan == Scan::Settle {
            let threshold_met = !CONTRAST_RATIO.below(point.score.0, self.libcamera.maximum.0)
                && self.libcamera.minimum.is_some_and(|minimum|
                    u64::from(minimum.0) * u64::from(CONTRAST_RATIO.denominator)
                        <= u64::from(self.libcamera.maximum.0) * u64::from(CONTRAST_RATIO.numerator));
            if !self.libcamera_requires_scene() {
                return self.terminal(if threshold_met { Action::Finish(point.position) }
                    else { Action::Fail(Failure::LowConfidence) });
            }
            self.libcamera.scan = Scan::Monitor(if threshold_met { FocusStatus::Focused } else { FocusStatus::Failed });
            self.libcamera.scene_contrast = Score(self.libcamera.maximum.0.max(point.score.0));
            self.libcamera.stable = Frames(0);
            return self.libcamera_issue();
        }
        let index = usize::from(self.count - 1);
        self.libcamera.focuses[index] = self.libcamera.target;
        if index == self.libcamera.scan_start || point.score.0 > self.libcamera.maximum.0 {
            self.libcamera.maximum = point.score;
            self.libcamera.best_index = index;
            if self.libcamera.scan != Scan::Fine {
                if let Some(means) = means { self.libcamera.scene_means = means; }
            }
        }
        self.libcamera.minimum = Some(self.libcamera.minimum.map_or(point.score,
            |minimum| Score(minimum.0.min(point.score.0))));
        let at_limit = (self.libcamera.step.0 > 0 && self.libcamera.target == FOCUS_MAX)
            || (self.libcamera.step.0 < 0 && self.libcamera.target == FOCUS_MIN);
        let fine_complete = self.libcamera.scan == Scan::Fine
            && usize::from(self.count) - self.libcamera.scan_start >= FINE_SAMPLES;
        if at_limit || fine_complete || CONTRAST_RATIO.below(point.score.0, self.libcamera.maximum.0) {
            let peak = self.libcamera_peak();
            let first = self.points[self.libcamera.scan_start].score;
            if self.libcamera.scan == Scan::CoarseFirst && !CONTRAST_RATIO.below(first.0, self.libcamera.maximum.0) {
                self.libcamera.step = LensStep(-self.libcamera.step.0);
                self.libcamera.scan = Scan::CoarseReturn;
            } else if self.libcamera.scan == Scan::Fine {
                self.libcamera.target = peak;
                self.libcamera.scan = Scan::Settle;
            } else {
                let direction = if self.libcamera.step.0 > 0 { 1 } else { -1 };
                self.libcamera.target = peak.offset(LensStep(direction * FINE.0));
                self.libcamera.step = LensStep(-direction * FINE.0);
                self.libcamera.scan = Scan::Fine;
            }
            self.libcamera.scan_start = usize::from(self.count);
        } else { self.libcamera.target = self.libcamera.target.offset(self.libcamera.step); }
        self.libcamera.delay = if self.libcamera.target == self.libcamera.smooth { Frames(0) } else { STEP_FRAMES };
        self.libcamera_issue()
    }
    fn libcamera_monitor(&mut self, contrast: Score, means: RgbMeans) -> Action {
        let changed = RETRIGGER_RATIO.scene_changed(contrast.0, self.libcamera.scene_contrast.0)
            || means.0.iter().zip(self.libcamera.scene_means.0.iter())
                .any(|(current, reference)| RETRIGGER_RATIO.scene_changed(current.0, reference.0));
        if changed {
            self.libcamera.scene_contrast = contrast;
            self.libcamera.scene_means = means;
            self.libcamera.stable = Frames(1);
        } else if self.libcamera.stable.0 > 0 { self.libcamera.stable.0 += 1; }
        if self.libcamera.stable.0 >= RETRIGGER_DELAY.0 { self.libcamera_programmed_scan() }
        else { self.libcamera_issue() }
    }
}
