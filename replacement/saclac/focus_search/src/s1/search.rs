#![forbid(unsafe_code)]

//! The S1 search's state and the steps its three phases share.

use super::sweep::Sweep;
use super::track::Track;
use super::{climb::Climb, S1Ports};
use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{travel_limits, FitPoint, LensIdentity, Platform, StepQuery, TravelLimits};
use crate::intake::{RecordReads, Zone};
use xm1_saclac_controllers::search::LensPosition;
use xm1_saclac_interface::search::{BridgeReport, BridgeStatus, Evaluation, S1DriveRequest, ScanEvaluation, SearchRecord};

/// A phase that steps with each evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stepping {
    /// Steps toward rising contrast until the trend shows where the peak lies.
    Climb,
    /// Follows a peak the climb found.
    Track,
    /// Runs through the travel range until the contrast has risen and fallen.
    Sweep,
}

/// The phase a step leaves the search in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum S1Phase {
    Climb,
    Track,
    Sweep,
    /// Drive to the best position found.
    Conclude,
}

impl S1Phase {
    pub(super) const fn stepping(self) -> Option<Stepping> {
        match self {
            Self::Climb => Some(Stepping::Climb),
            Self::Track => Some(Stepping::Track),
            Self::Sweep => Some(Stepping::Sweep),
            Self::Conclude => None,
        }
    }
}

impl From<Stepping> for S1Phase {
    fn from(stepping: Stepping) -> Self {
        match stepping {
            Stepping::Climb => Self::Climb,
            Stepping::Track => Self::Track,
            Stepping::Sweep => Self::Sweep,
        }
    }
}

/// The record fields the S1 search starts from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S1Record {
    /// 0x74ec30.
    pub scale: u32,
    /// 0x74e5c0.
    pub selection: u32,
    /// 0x74fa2c.
    pub mode: u32,
    /// 0x74e648.
    pub profile_class: u32,
    /// The first word 0x758c98 copies.
    pub resource: u32,
    /// 0x75702c.
    pub near: i16,
    /// 0x75707c.
    pub far: i16,
    /// 0x756fe4.
    pub step: u32,
    /// 0x757254.
    pub timing: u32,
}

/// What the capture pipeline recorded with the S1 frame (0xfeaadc(1)).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S1Snapshot {
    pub size: [u32; 2],
    /// The lens-table row the frame was taken with.
    pub lens: LensIdentity,
    /// The window for a frame mode other than 1, then the one for mode 1.
    pub windows: [[u32; 2]; 2],
    pub extent: [u32; 2],
    pub frame_mode: u32,
    pub pinned_setting: u16,
}

/// What the search keeps from the frame it last sent for evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Frame {
    pub mode: u32,
    /// The frame mode when the phase was last set.
    pub phase_mode: u32,
    pub lens: LensIdentity,
    pub pinned_setting: u16,
    /// 0x750474.
    pub interval_tracking: u32,
    /// The record's near position.
    pub near: i16,
}

/// The evaluation the search last took in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Values {
    /// The first pair's second sum (word 1) and its first sum (word 0).
    pub seconds: u32,
    pub combined: u32,
    pub reference: u32,
    pub settled: u32,
    pub flag: u32,
    /// The channel's sum the phase steps by.
    pub contrast: u32,
}

/// The best position found and the drive speed the search goes there with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Best {
    pub contrast: u32,
    pub position: LensPosition,
    pub speed: u32,
}

/// The contrast of the sample a phase steps from, the one before, and where each was taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Trend {
    pub contrast: u32,
    pub previous: u32,
    /// `contrast` less `previous`, and that change one sample earlier.
    pub change: u32,
    pub previous_change: u32,
    pub position: LensPosition,
    pub previous_position: LensPosition,
    pub class: u32,
}

impl Trend {
    const START: Self = Self {
        contrast: 0,
        previous: 0,
        change: 0,
        previous_change: 0,
        position: LensPosition(0),
        previous_position: LensPosition(0),
        class: 2,
    };

    pub(super) const fn moved(&self) -> bool { self.position.0 != self.previous_position.0 }
}

/// The samples around a new best that a peak is fitted through, and the newest sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FitWindow {
    before: FitPoint,
    best: FitPoint,
    after: FitPoint,
    newest: FitPoint,
}

impl FitWindow {
    const EMPTY: Self = Self { before: FitPoint::EMPTY, best: FitPoint::EMPTY, after: FitPoint::EMPTY, newest: FitPoint::EMPTY };

    pub(super) fn forget(&mut self) { *self = Self::EMPTY; }

    pub(super) fn note_newest(&mut self, contrast: u32, position: LensPosition) { self.newest = FitPoint { contrast, position: position.0 }; }

    pub(super) fn note_best(&mut self, contrast: u32, position: LensPosition) {
        self.before = self.newest;
        self.best = FitPoint { contrast, position: position.0 };
        self.after = FitPoint::EMPTY;
    }

    /// Takes the sample after a best; true when the three bracket an extreme along a
    /// monotone run of positions. Contrasts compare as signed words.
    pub(super) fn complete(&mut self, platform: &mut impl Platform, contrast: u32, position: LensPosition) -> bool {
        if self.after.contrast != u32::MAX || self.best.contrast == u32::MAX {
            return false;
        }
        self.after = FitPoint { contrast, position: position.0 };
        if platform.setting(Setting::FitPeak) == 0 || self.before.contrast == u32::MAX {
            return false;
        }
        let [before, best, after] = [self.before.contrast as i32, self.best.contrast as i32, self.after.contrast as i32];
        if !((before > best && after > best) || (before < best && after < best)) {
            return false;
        }
        let points = [self.before.position, self.best.position, self.after.position];
        if points.contains(&FitPoint::EMPTY.position) {
            return false;
        }
        (points[0] < points[1] && points[1] < points[2]) || (points[0] > points[1] && points[1] > points[2])
    }

    pub(super) fn fit(&self, platform: &mut impl Platform) -> LensPosition { platform.fit_peak([self.before, self.best, self.after]) }
}

pub(super) struct Search {
    /// `None` before the first initialization and after the search finished.
    pub phase: Option<S1Phase>,
    /// An evaluation was taken since the search was initialized.
    evaluated: bool,
    pub samples: u8,
    pub record: SearchRecord,
    record_step: u32,
    timing: u32,
    pub frame: Frame,
    pub values: Values,
    pub limits: TravelLimits,
    /// The retained focus lies beyond the travel range, so the record's own table bounds it.
    beyond_high: bool,
    pub step: u16,
    pub rate: u32,
    pub speed: u32,
    pub direction: i8,
    pub previous_direction: i8,
    /// Newest first: the lens position at this sample and the two before.
    pub positions: [LensPosition; 3],
    pub reported: LensPosition,
    /// Newest first: the lens classes at those positions.
    pub classes: [u32; 3],
    pub reported_class: u32,
    pub report_status: BridgeStatus,
    pub best: Best,
    pub trend: Trend,
    pub climb: Climb,
    pub track: Track,
    pub fit: FitWindow,
    pub sweep: Sweep,
}

impl Search {
    pub const fn new() -> Self {
        Self {
            phase: None,
            evaluated: false,
            samples: 0,
            record: SearchRecord {
                first: false,
                scale: [0; 2],
                settled: false,
                level_matched: false,
                selection: 0,
                mode: 0,
                ratio: [0; 2],
                range: [0; 2],
                range_offset: 0,
                extent: [0; 2],
                window: [0; 2],
                frame_mode: 0,
                scan: false,
                profile_class: 0,
                resource: 0,
                scan_evaluation: ScanEvaluation::Plain,
                size: [0; 2],
                timing: 0,
                decade: 0,
                correction: 0,
                limit: 0,
            },
            record_step: 0,
            timing: 0,
            frame: Frame { mode: 0, phase_mode: 0, lens: LensIdentity(0), pinned_setting: 0, interval_tracking: 0, near: 0 },
            values: Values { seconds: 0, combined: 0, reference: 0, settled: 0, flag: 0, contrast: 0 },
            limits: TravelLimits {
                low: LensPosition(0),
                high: LensPosition(0),
                inner_low: LensPosition(0),
                inner_high: LensPosition(0),
                offset: 0,
            },
            beyond_high: false,
            step: 0,
            rate: 0,
            speed: 0,
            direction: 0,
            previous_direction: 0,
            positions: [LensPosition(0); 3],
            reported: LensPosition(0),
            classes: [0; 3],
            reported_class: 0,
            report_status: BridgeStatus::Moved,
            best: Best { contrast: 0, position: LensPosition(0), speed: 0 },
            trend: Trend::START,
            climb: Climb::START,
            track: Track::START,
            fit: FitWindow::EMPTY,
            sweep: Sweep::new(),
        }
    }

    /// 0xf26a60: forget the last search and choose where the next one starts, or `None` when
    /// the record does not run the S1 search.
    pub fn initialize(&mut self, ports: &mut impl S1Ports) -> Option<Stepping> {
        let record = ports.record();
        if ports.setting(Setting::S1Search) == 0 || record.selection != 0 || !(4..=6).contains(&record.mode) {
            return None;
        }
        *self = Self::new();
        let ratio = ports.selection_ratio();
        let lens = ports.lens_identity();
        let level = ports.lens_level(record.step, lens);
        let base = ports.zone_limit(Zone(3));
        let zone = zone_of(base, ports);
        let range_offset = record.far.wrapping_sub(ports.zone_limit(zone));
        let range_end = level.wrapping_add(record.near).wrapping_sub(range_offset);
        self.record = SearchRecord {
            scale: [record.scale as u16, (record.scale >> 16) as u16],
            selection: record.selection,
            mode: record.mode,
            ratio,
            range: [range_end, range_end],
            range_offset,
            profile_class: record.profile_class,
            resource: record.resource,
            ..self.record
        };
        self.record_step = record.step;
        self.timing = record.timing;
        self.restart_at(ports.lens_position());
        let retained = ports.retained_focus();
        self.update_limits(ports);
        self.beyond_high = retained.position > self.limits.high;
        let start = if retained.spread > 1 { Stepping::Climb } else { Stepping::Track };
        self.phase = Some(start.into());
        Some(start)
    }

    /// 0xf25784: search again from where the lens is, climbing.
    pub fn restart(&mut self, platform: &mut impl Platform) {
        self.samples = 0;
        self.restart_at(platform.lens_position());
        self.phase = Some(S1Phase::Climb);
    }

    fn restart_at(&mut self, position: LensPosition) {
        self.best.position = position;
        self.positions[0] = position;
        self.positions[1] = position;
        self.reported = position;
        self.direction = 1;
        self.previous_direction = 1;
        self.classes = [2; 3];
        self.reported_class = 2;
    }

    /// 0xf2711c: stage the frame's statistics and fill the calc record for it.
    pub fn frame(&mut self, ports: &mut impl S1Ports) -> SearchRecord {
        ports.stage_statistics();
        let snapshot = ports.frame_snapshot();
        let decade = ports.converted_decade();
        let timing = ports.record().timing;
        let near = ports.near_position();
        let wide = 10 * ports.setting(Setting::WideRangeTens) as i32;
        let interval_tracking = ports.interval_tracking();
        let far = self.record.range[1];
        let sweeping = self.phase == Some(S1Phase::Sweep);
        let scan = match snapshot.frame_mode {
            _ if !sweeping => false,
            0 if self.samples == 0 => self.record.scan || newest_levels_high(ports),
            1 => false,
            _ => self.record.scan,
        };
        self.record = SearchRecord {
            settled: true,
            level_matched: far > near && i32::from(far) - i32::from(near) > wide,
            range: [near, far],
            extent: snapshot.extent,
            window: if snapshot.frame_mode == 1 { snapshot.windows[1] } else { snapshot.windows[0] },
            frame_mode: snapshot.frame_mode,
            scan,
            scan_evaluation: match (sweeping, scan) {
                (true, false) => ScanEvaluation::Scan,
                (true, true) => ScanEvaluation::Scanned,
                (false, _) => ScanEvaluation::Plain,
            },
            size: snapshot.size,
            timing,
            decade,
            correction: 0,
            limit: far,
            ..self.record
        };
        self.frame = Frame {
            mode: snapshot.frame_mode,
            lens: snapshot.lens,
            pinned_setting: snapshot.pinned_setting,
            interval_tracking,
            near,
            ..self.frame
        };
        self.record
    }

    /// 0xf267f8.
    pub fn take_evaluation(&mut self, evaluation: &Evaluation, platform: &mut impl Platform) {
        let [[combined, seconds], _, _] = evaluation.pairs;
        self.values = Values {
            seconds,
            combined,
            reference: evaluation.reference,
            settled: evaluation.settled,
            flag: evaluation.flag,
            ..self.values
        };
        if !self.evaluated {
            self.evaluated = true;
            if self.phase == Some(S1Phase::Track) {
                self.select_channel(platform);
                self.best.contrast = self.values.contrast;
            }
        }
        self.positions = [self.reported, self.positions[0], self.positions[1]];
        self.previous_direction = self.direction;
        self.classes = [self.reported_class, self.classes[0], self.classes[1]];
        self.samples = match self.samples.wrapping_add(1) {
            255 => 254,
            samples => samples,
        };
    }

    /// 0xf25b54.
    pub fn take_report(&mut self, report: BridgeReport) {
        self.report_status = report.status;
        let position = LensPosition(report.position);
        self.reported = position;
        self.reported_class = report.class.0;
        self.positions[0] = position;
        self.classes[0] = report.class.0;
    }

    /// 0xf2580c, 0xf25860 and the phase's step.
    pub fn step(&mut self, stepping: Stepping, platform: &mut impl Platform) -> S1Phase {
        self.update_limits(platform);
        self.select_channel(platform);
        match stepping {
            Stepping::Climb => self.climb(platform),
            Stepping::Track => self.track(platform),
            Stepping::Sweep => self.sweep(platform),
        }
    }

    /// 0xf2580c.
    fn update_limits(&mut self, platform: &mut impl Platform) {
        if let Some(limits) = travel_limits(platform, self.beyond_high) {
            self.limits = limits;
        }
    }

    /// 0xf25860.
    fn select_channel(&mut self, platform: &mut impl Platform) {
        let seconds = self.frame.mode == 0 && {
            let setting = if self.phase == Some(S1Phase::Climb) { Setting::DirectionSecondChannel } else { Setting::SecondChannel };
            platform.setting(setting) == 1
        };
        self.values.contrast = if seconds { self.values.seconds } else { self.values.combined };
    }

    /// 0xf26ff0.
    pub fn set_phase(&mut self, phase: S1Phase) {
        if self.phase != Some(phase) {
            self.samples = 0;
        }
        self.phase = Some(phase);
        self.frame.phase_mode = self.frame.mode;
    }

    /// 0xf267b0: after the lens moved to the best position, track it.
    pub fn rearm(&mut self) -> bool {
        if self.report_status != BridgeStatus::Moved {
            return false;
        }
        self.samples = 0;
        self.phase = Some(S1Phase::Track);
        true
    }

    /// 0xf26fbc.
    pub fn finish(&mut self) { self.phase = None; }

    /// 0xf258e0: head for the best position, held inside the travel range.
    pub fn aim_at_best(&mut self) {
        let target = if self.best.position < self.limits.low {
            self.best.speed = 2;
            self.limits.low
        } else if self.best.position > self.limits.high {
            self.best.speed = 2;
            self.limits.high
        } else {
            self.best.position
        };
        self.best.position = target;
        self.direction = if target > self.positions[0] { 1 } else { -1 };
        self.step = target.0.wrapping_sub(self.positions[0].0).wrapping_mul(i16::from(self.direction)) as u16;
        self.speed = self.best.speed;
    }

    /// 0xf259b8.
    pub fn bound_step(&mut self, platform: &mut impl Platform) {
        let position = self.positions[0].0;
        let (low, high) = (self.limits.low.0, self.limits.high.0);
        let step = self.step as i16;
        if position < low || position > high {
            let distance = if position < low {
                self.direction = 1;
                low.wrapping_sub(position) as u16
            } else {
                self.direction = -1;
                position.wrapping_sub(high) as u16
            };
            let limit = platform.setting(Setting::ReturnStepLimit);
            let distance = if u32::from(distance) > limit { limit as u16 } else { distance };
            if step < distance as i16 {
                self.step = distance;
            }
        } else if self.direction == -1 && i32::from(position) - i32::from(step) < i32::from(low) {
            self.step = position.wrapping_sub(low) as u16;
        } else if self.direction == 1 && i32::from(position) + i32::from(step) > i32::from(high) {
            self.step = high.wrapping_sub(position) as u16;
        }
    }

    /// 0xf25be0: `moves` drives by the step, otherwise the lens is released where it is.
    pub fn drive_request(&self, moves: bool, platform: &mut impl Platform) -> S1DriveRequest {
        let travel = if moves { (self.step as i16).wrapping_mul(i16::from(self.direction)) } else { 0 };
        let speed = if moves { self.speed } else { 2 };
        let lens_mode = platform.lens_mode();
        let speed = if platform.setting(Setting::SlowBridge) == 1 { 2 } else { speed };
        S1DriveRequest { travel, rate: self.rate, moves, speed, lens_mode }
    }

    /// 0xfaab3c for `kind` at the channel contrast; the rate goes with the next drive.
    pub fn step_for(&mut self, platform: &mut impl Platform, kind: StepKind) -> u16 {
        let lens_mode = platform.lens_mode();
        let step = platform.step(StepQuery {
            kind,
            parameter: self.record_step,
            contrast: self.values.contrast,
            profile_class: self.timing,
            lens_mode,
        });
        self.rate = step.rate;
        step.size
    }

    /// 0xf2745c.
    pub fn begin_sample(&mut self) {
        if self.samples == 1 {
            self.track = Track::START;
            self.trend = Trend::START;
            self.climb.restart();
            self.report_status = BridgeStatus::Moved;
            self.fit.forget();
        }
        self.trend.contrast = self.values.contrast;
        self.trend.position = self.positions[2];
        self.trend.class = self.classes[2];
    }

    /// 0xf2736c: at a travel limit, head back inside.
    pub fn turn_at_limits(&mut self) {
        let position = self.positions[0];
        if self.limits.low >= position {
            self.direction = 1;
        } else if self.limits.high <= position {
            self.direction = -1;
        }
    }

    /// 0xf273d4.
    pub fn shift_trend(&mut self) {
        self.trend.previous_change = self.trend.change;
        self.trend.previous = self.trend.contrast;
        self.trend.previous_position = self.trend.position;
    }

    /// 0xf280c0.
    pub fn halt(&mut self) {
        self.step = 0;
        self.samples = 0;
    }

    /// 0xf283b4: halves a step that would carry the lens past the limit ahead.
    pub fn limit_step(&mut self) {
        let position = self.positions[0].0;
        let room = if self.direction == 1 { self.limits.high.0.wrapping_sub(position) } else { position.wrapping_sub(self.limits.low.0) } as u16;
        let step = i32::from(self.step as i16);
        if i32::from(room) < step * 2 {
            let half = ((u32::from(room) + 1) >> 1) as u16;
            if step > i32::from(half as i16) {
                self.step = half;
            }
        }
    }

    /// The bits of `ProbeDirection` are probe directions, set for 1.
    pub fn probe_direction(platform: &mut impl Platform, bit: u32) -> i8 {
        if platform.setting(Setting::ProbeDirection) & bit != 0 { 1 } else { -1 }
    }
}

/// 0xf27040: bit 0 of `RecentPeak` lets the newest measurement start the sweep on the scan
/// pairs when either of its levels reaches `RecentPeakLevel`.
fn newest_levels_high(platform: &mut impl Platform) -> bool {
    if platform.setting(Setting::RecentPeak) & 1 == 0 {
        return false;
    }
    let Some([Some(levels), ..]) = platform.measurement_history() else {
        return false;
    };
    let level = platform.setting(Setting::RecentPeakLevel);
    levels.iter().any(|value| u32::from(*value) >= level)
}

/// 0x772ff8: the zone below the first of zones 2 to 10 whose limit lies above `limit`, zone 10
/// when none does, and zone 0 for a zero limit.
fn zone_of(limit: i16, reads: &mut impl RecordReads) -> Zone {
    if limit == 0 {
        return Zone(0);
    }
    (2..=10).find(|&zone| limit < reads.zone_limit(Zone(zone))).map_or(Zone(10), |zone| Zone(zone - 1))
}
