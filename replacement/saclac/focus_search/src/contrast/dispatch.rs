#![forbid(unsafe_code)]

//! Machine 23's searching states: what each lane input, intake, lens report and evaluation
//! does in the stage the search is in, and what it asks of the lens bridge.

use super::sample::{Range, Stage};
use super::{BridgeDrive, Context, Phase, Platform, ScanFrame};
use crate::intake::{self, Intake, IntakePorts, IntakeView};
use crate::live::{LiveStrategy, Measurement};
use xm1_saclac_controllers::search::{FocusSearch, LensPosition, SearchDecision, SearchInput, SearchStep, SearchTerminal};
use xm1_saclac_interface::search::{BridgeReport, Evaluation, SearchRecord};

/// Stock X-M1 1.50's contrast search as a strategy of the live search.
pub struct ContrastSearch<P> {
    platform: P,
}

impl<P> ContrastSearch<P> {
    pub const fn new(platform: P) -> Self { Self { platform } }

    pub fn platform(&self) -> &P { &self.platform }

    pub fn platform_mut(&mut self) -> &mut P { &mut self.platform }
}

impl<P: Platform + IntakePorts> FocusSearch for ContrastSearch<P> {
    type Context = Context;
    type Sample = Measurement<Evaluation>;
    type Drive = BridgeDrive;

    fn advance(&mut self, context: &mut Context, input: SearchInput<Self::Sample>) -> SearchDecision<BridgeDrive> {
        let platform = &mut self.platform;
        SearchDecision::Next(match input {
            SearchInput::Initialize => {
                context.initialize(platform);
                SearchStep::Measure
            }
            SearchInput::Begin => {
                context.begin(platform);
                SearchStep::Measure
            }
            SearchInput::Sample(measurement) => context.sample(platform, &measurement.evaluation, measurement.bridge_busy),
            SearchInput::Suspend => {
                context.stage = Stage::Monitor;
                context.begin_monitor();
                SearchStep::Measure
            }
            SearchInput::Resume => {
                context.stage = Stage::Direction;
                context.resume(platform);
                SearchStep::Measure
            }
            SearchInput::Cancel => context.cancel(platform),
            SearchInput::ResumePending => context.cancel_watch(platform),
        })
    }
}

impl<P: Platform + IntakePorts> LiveStrategy for ContrastSearch<P> {
    type Evaluation = Evaluation;
    type Record = SearchRecord;

    fn ready(&mut self, context: &Context, bridge_busy: bool) -> bool { context.ready(&mut self.platform, bridge_busy) }

    fn take_in(&mut self, context: &mut Context) -> SearchRecord {
        let view = IntakeView {
            phase: context.phase,
            accepted: context.accepted,
            samples: context.samples,
            scan: context.recent_peak,
            focus_value: context.values.focus_raw as i16,
        };
        let intake = intake::take_in(view, &mut self.platform);
        context.keep_intake(&intake);
        intake.record
    }

    fn lens_reported(&mut self, context: &mut Context, report: BridgeReport) {
        context.bridge.last_report = report.status;
        context.reported = LensPosition(report.position);
        context.reported_class = report.class.0;
    }

    fn release(&mut self, context: &Context) -> BridgeDrive { context.drive(&mut self.platform, false).1 }

    fn accepts_begin(&self, context: &Context) -> bool { matches!(context.stage, Stage::Direction | Stage::Focus) }

    fn finished(&mut self, _: &mut Context, _: SearchTerminal) {}
}

impl Context {
    /// Machine 23's Enable (0xf0c9c4): start in focus or in direction search, then initialize.
    fn begin(&mut self, platform: &mut impl Platform) {
        let start = self.choose_start(platform);
        self.stage = if start == Phase::Focus { Stage::Focus } else { Stage::Direction };
        self.initialize(platform);
    }

    /// What the intake took in with the frame (0xf40944's context and record stores).
    fn keep_intake(&mut self, intake: &Intake) {
        let taken = &intake.frame;
        self.lens = taken.lens;
        let frame = &mut self.frame;
        frame.selected = taken.frame_mode;
        frame.status_flag = taken.selected;
        if let Some(near) = taken.settled_near {
            frame.watch = near;
        }
        frame.parameter = taken.step;
        frame.levels = taken.levels;
        frame.scan_allowed = taken.stage_first_pass != 0;
        frame.scan = ScanFrame {
            parameter_index: taken.parameter_index,
            stage_halves: taken.stage_halves,
            current_lens: taken.current_lens.0 as u32,
            stage_last: taken.stage_last,
        };
        frame.pinned = taken.pinned;
        frame.pinned_setting = taken.pinned_setting;
        frame.mark = taken.mark;
        frame.profile_class = taken.imaging_profile_class;
        frame.correction_source = taken.correction_source;
        frame.correction_class = taken.correction_class;
        frame.corrects_position = taken.corrects_position != 0;
        frame.scan_flag = u8::from(taken.scan);
        frame.record_mode = intake.record.mode;
        frame.step_class = u32::from(intake.record.level_matched);
        self.pacing[..2].copy_from_slice(&taken.pacing);
        self.recent_peak = taken.scan;
    }

    /// Stop or suspend with requests in flight (0xf0ca70, 0xf0cae8).
    fn cancel(&mut self, platform: &mut impl Platform) -> SearchStep<BridgeDrive> {
        let scanning = match self.stage {
            Stage::PeakSearch => true,
            Stage::Monitor => !self.monitor.following_lens(),
            _ => false,
        };
        if !scanning {
            return SearchStep::Measure;
        }
        self.stop_scan(platform);
        self.interrupt_scan()
    }

    /// Resume with requests in flight (0xf0cc60, 0xf0cce0).
    fn cancel_watch(&mut self, platform: &mut impl Platform) -> SearchStep<BridgeDrive> {
        if self.monitor.following_lens() {
            return SearchStep::Measure;
        }
        self.stop_scan(platform);
        self.interrupt_scan()
    }

    /// Machine 23's evaluation actions (row 6): accept the evaluation, step the phase and ask
    /// the bridge for what the stage the step leads to needs.
    fn sample(&mut self, platform: &mut impl Platform, evaluation: &Evaluation, bridge_busy: bool) -> SearchStep<BridgeDrive> {
        if self.stage == Stage::Idle {
            return SearchStep::Measure;
        }
        self.accept(platform, evaluation);
        let range = self.check_range(platform);
        if self.stage == Stage::Monitor {
            self.choose_channel(platform);
            let step = self.monitor_step(platform, bridge_busy);
            self.enter(platform, false, step);
            return self.follow_lens(platform);
        }
        if range == Range::NoLens {
            self.stage = Stage::Direction;
            return SearchStep::Measure;
        }
        self.choose_channel(platform);
        let chosen = match self.stage {
            Stage::Direction => self.direction_step(platform),
            Stage::PeakSearch => self.peak_search_step(platform),
            Stage::Peak | Stage::PeakMoved => self.peak_step(platform),
            Stage::Return => self.return_step(platform),
            Stage::Focus => self.focus_step(platform),
            Stage::Approach => self.approach_step(platform),
            Stage::FocusDrive | Stage::FocusDriveMoved => self.focus_drive_step(platform),
            Stage::Idle | Stage::Monitor => return SearchStep::Measure,
        };
        let restart = self.take_restart();
        let next = self.enter(platform, restart, chosen);
        self.advance_stage(platform, next)
    }

    /// The stage `next` leads to from the current one, and its request.
    fn advance_stage(&mut self, platform: &mut impl Platform, next: Phase) -> SearchStep<BridgeDrive> {
        use Phase as P;
        use Stage as S;
        let (stage, request) = match (self.stage, next) {
            (S::Direction, P::PeakSearch) => (S::PeakSearch, Request::None),
            (S::Direction, P::Return) => (S::Return, Request::None),
            (S::Direction, P::FocusDrive) => (S::FocusDrive, Request::None),
            (S::Direction, _) => (S::Direction, Request::Travel),
            (S::PeakSearch, P::Direction) => (S::Direction, Request::StopScan),
            (S::PeakSearch, P::Peak) => (S::Peak, Request::StopScan),
            (S::PeakSearch, P::FocusDrive) => (S::FocusDrive, Request::None),
            (S::PeakSearch, _) => (S::PeakSearch, Request::Scan),
            (S::Peak | S::PeakMoved, P::Direction) => (S::Direction, Request::None),
            (S::Peak | S::PeakMoved, P::Focus) => (S::Focus, Request::Travel),
            (S::Peak | S::PeakMoved, P::Return) => (S::Return, Request::None),
            (S::Peak | S::PeakMoved, P::FocusDrive) => (S::FocusDrive, Request::None),
            (S::Peak | S::PeakMoved, _) => (S::PeakMoved, Request::Travel),
            (S::Return | S::Approach, P::Direction) => (S::Direction, Request::None),
            (S::Return | S::Approach, P::PeakSearch) => (S::PeakSearch, Request::None),
            (S::Return | S::Approach, P::FocusDrive) => (S::FocusDrive, Request::None),
            (S::Return | S::Approach, _) => (self.stage, Request::Travel),
            (S::Focus, P::Direction) => (S::Direction, Request::None),
            (S::Focus, P::Approach) => (S::Approach, Request::None),
            (S::Focus, P::Peak) => (S::Peak, Request::None),
            (S::Focus, P::FocusDrive) => (S::FocusDrive, Request::None),
            (S::Focus, _) => (S::Focus, Request::None),
            (S::FocusDrive | S::FocusDriveMoved, P::Direction) => (S::Direction, Request::None),
            (S::FocusDrive | S::FocusDriveMoved, P::Focus) => (S::Focus, Request::Travel),
            (S::FocusDrive | S::FocusDriveMoved, _) => (S::FocusDriveMoved, Request::Travel),
            (S::Idle | S::Monitor, _) => (self.stage, Request::None),
        };
        self.stage = stage;
        match request {
            Request::None => SearchStep::Measure,
            Request::Travel => self.travel(platform),
            Request::StopScan => self.interrupt_scan(),
            Request::Scan if self.scan_in_motion() => SearchStep::Interrupt,
            Request::Scan if self.scan_continues() => SearchStep::Measure,
            Request::Scan => self.travel(platform),
        }
    }

    /// The monitor's request (0xf0d7d0): once the lens moved, a return travels and a peak
    /// search scans until it settles.
    fn follow_lens(&mut self, platform: &mut impl Platform) -> SearchStep<BridgeDrive> {
        if self.monitor.moved.is_none() {
            SearchStep::Measure
        } else if self.monitor.following_lens() {
            self.travel(platform)
        } else if self.scan_in_motion() {
            SearchStep::Interrupt
        } else if self.scan_continues() || self.monitor.settled {
            SearchStep::Measure
        } else {
            self.travel(platform)
        }
    }

    /// 0xf3d544 when the scan drive is still moving (0xf3eaf0).
    fn interrupt_scan(&self) -> SearchStep<BridgeDrive> { if self.scan_in_motion() { SearchStep::Interrupt } else { SearchStep::Measure } }
}

/// What a stage change asks of the bridge.
enum Request {
    None,
    Travel,
    /// Interrupt a scan still moving.
    StopScan,
    /// A scanning peak search interrupts a moving scan, waits for a planned one, or travels.
    Scan,
}
