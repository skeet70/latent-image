use crate::acquisition::{self, Pass};
use crate::fakes::{Outbound, Ports};
use crate::image::{ChannelCount, ImagePeer, LensReport};
use crate::jobs::{self, Calculation};
use crate::lanes::Selection;
use crate::live::{Effects, Measurement};
use crate::pipeline::{Statistics, TimingWord};
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{FrameInterval, FrameRate, ProfileSelection, WindowPosition, WindowStatus};
use crate::record::Request;
use crate::window::WindowResult;
use crate::round::{LaneSignal, LaneStatus};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl};
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Search(&'static str),
    Calculation(Calculation, jobs::Effect),
    Acquisition(acquisition::Effect),
    Image(&'static str),
    Fault,
}

struct Live { traffic: Vec<Traffic>, source_ready: bool, failed: bool, programmed: Option<TimingWord> }

impl Live {
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

type Fake = Ports<Live>;

fn fake(available: Selection) -> Fake { Ports::new(Some(available), Live { traffic: Vec::new(), source_ready: false, failed: false, programmed: None }) }

impl Effects for Fake {
    fn fault(&mut self) { self.extra.traffic.push(Traffic::Fault); }
    fn search(&mut self, signal: LaneSignal<'_>, _: RecordHandle) -> LaneStatus {
        let (name, status) = match signal {
            LaneSignal::Work(_) => ("work", LaneStatus::Pending),
            LaneSignal::Initialize => ("initialize", LaneStatus::Complete),
            LaneSignal::Enable => ("enable", LaneStatus::Complete),
            LaneSignal::Stop => ("stop", LaneStatus::Complete),
            _ => ("other", LaneStatus::Complete),
        };
        self.extra.traffic.push(Traffic::Search(name));
        status
    }
    fn calculation(&mut self, _: RecordHandle, calculation: Calculation, effect: jobs::Effect) { self.extra.traffic.push(Traffic::Calculation(calculation, effect)); }
    fn source_available(&mut self, _: RecordHandle) -> bool { self.extra.source_ready }
    fn result_changed(&mut self, _: RecordHandle, _: Calculation) -> bool { false }
    fn acquisition(&mut self, _: RecordHandle, effect: acquisition::Effect) -> Option<TimingWord> {
        self.extra.traffic.push(Traffic::Acquisition(effect));
        if effect == acquisition::Effect::Initialize { self.extra.programmed } else { None }
    }
    fn selection_changed(&mut self, _: RecordHandle) -> bool { false }
    fn image(&mut self, _: RecordHandle, request: ImagePeer<'_>) {
        self.extra.traffic.push(Traffic::Image(match request {
            ImagePeer::Initialize => "initialize",
            ImagePeer::PrepareGeometry => "geometry",
            ImagePeer::Request { .. } => "request",
            ImagePeer::Cancel(_) => "cancel",
            ImagePeer::Stop => "stop",
        }));
    }
    fn image_failed(&mut self, _: Completed<'_>) -> bool { self.extra.failed }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }
fn control(control: MeasurementControl) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}
fn completion(completion: Completion) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(&[]) }, record: record() }
}

fn command(words: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Command { command: Command { kind: CommandKind::Command14, parameters: stock::parameters(words) }, record: record() }
}

const ACQUISITION: Selection = Selection { acquisition: true, search: false, auxiliary: false, image: false, source: false };

#[test]
fn initialization_enables_acquisition_and_window_and_initializes_the_other_lanes() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [
        Traffic::Acquisition(acquisition::Effect::Initialize),
        Traffic::Acquisition(acquisition::Effect::Enable),
        Traffic::Search("initialize"),
        Traffic::Calculation(Calculation::Auxiliary, jobs::Effect::Initialize),
        Traffic::Image("initialize"),
        Traffic::Calculation(Calculation::Source, jobs::Effect::Initialize),
    ]);
    assert!(fake.records.in_use.primary);
}

#[test]
fn a_round_submits_window_and_primary_acquisition_and_collects_their_results() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [Traffic::Acquisition(acquisition::Effect::SubmitPrimary(Pass::First))]);
    assert!(fake.take().iter().any(|traffic| matches!(traffic, Outbound::Request(Request::Window(_)))));
    fake.window_result = Some(WindowResult { status: WindowStatus(5), position: WindowPosition([1, 2]) });
    assert_eq!(measurement.handle(completion(Completion::Calculation8d), &mut fake), ChildOutcome::Pending);
    assert_eq!((fake.records.window.status, fake.records.window_position), (WindowStatus(5), WindowPosition([1, 2])));
    assert_eq!(measurement.handle(completion(Completion::Calculation87), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [Traffic::Acquisition(acquisition::Effect::CollectPrimary), Traffic::Acquisition(acquisition::Effect::PublishPrimary)]);
}

#[test]
fn stopping_a_round_releases_each_enabled_job() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [Traffic::Acquisition(acquisition::Effect::Release), Traffic::Search("stop")]);
    assert!(fake.take().is_empty());
}

#[test]
fn an_image_command_replies_through_the_image_lane() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { image: true, ..ACQUISITION });
    fake.channel_count = ChannelCount(8);
    fake.lens = LensReport { first: true, second: false, third: true };
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    fake.take();
    let image = |channel: u32| -> Vec<u8> { [0xd00, 0, 0, channel, 9].iter().flat_map(|word: &u32| word.to_le_bytes()).collect() };
    let target = ReplyTarget::Command(CommandKind::Command14);
    let callback = stock::optional_callback(0xd00).unwrap();

    let words = image(5);
    assert_eq!(measurement.handle(command(&words), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [Traffic::Image("geometry"), Traffic::Image("request")]);
    fake.extra.failed = true;
    measurement.handle(completion(Completion::Image47), &mut fake);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback, status: CompletionStatus(-94), values: [5, 9, 6] },
    ]);

    let words = image(8);
    measurement.handle(command(&words), &mut fake);
    fake.extra.failed = false;
    measurement.handle(completion(Completion::Image47), &mut fake);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback, status: CompletionStatus(0), values: [8, 0x80_0000, 6] },
    ]);
}

#[test]
fn a_measurement_tick_captures_the_statistics_in_use_and_records_the_history() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.records.in_use.primary = true;
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    assert!(!fake.records.capture_attached);
    assert!(fake.records.history_restarted);
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    let traffic = fake.take();
    assert_eq!(traffic[..2], [Outbound::Capture(Statistics::Primary), Outbound::MeasurementHistory]);
    assert!(matches!(traffic[2], Outbound::Request(Request::Window(_))));
}

#[test]
fn a_parameter_change_with_nothing_to_change_answers_its_client() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    let block: Vec<u8> = [0xf0a0u32, 1, 1].iter().flat_map(|word| word.to_le_bytes()).collect();
    let change = ChildEvent::Command { command: Command { kind: CommandKind::Command17, parameters: stock::parameters(&block) }, record: record() };
    assert_eq!(measurement.handle(change, &mut fake), ChildOutcome::Complete);
    let target = ReplyTarget::Command(CommandKind::Command17);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback: stock::optional_callback(0xf0a0).unwrap(), status: CompletionStatus(0), values: [0; 3] },
    ]);
}

fn primary_in(mode: Mode, work: Work, current: Mode) -> Cadence {
    let admitted = work != Work::PrimaryAcquisition || current == mode;
    Cadence { first: admitted, second: admitted, third: admitted, fourth: admitted }
}
fn fast(work: Work, mode: Mode) -> Cadence { primary_in(Mode::AboveThreshold, work, mode) }
fn slow(work: Work, mode: Mode) -> Cadence { primary_in(Mode::BelowThreshold, work, mode) }
fn by_record(work: Work, mode: Mode) -> Cadence { primary_in(Mode::RecordSelected, work, mode) }
fn by_profile(work: Work, mode: Mode) -> Cadence { primary_in(Mode::ProfileSelected, work, mode) }

/// A round over a profile that paces by frame rate, after `configure`; whether its first tick submits primary acquisition.
fn submits_primary(configure: impl FnOnce(&mut Fake)) -> bool {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.records.profile.frame_rate_cadence = true;
    fake.frame_rate_threshold = FrameRate(30);
    fake.extra.programmed = Some(TimingWord(0x55));
    fake.set_current_interval(Some(FrameInterval(1_000)));
    configure(&mut fake);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    fake.extra.take().contains(&Traffic::Acquisition(acquisition::Effect::SubmitPrimary(Pass::First)))
}

#[test]
fn a_frame_rate_profile_paces_the_round_by_the_acquisitions_timing() {
    let paced = |interval: u32, cadence: fn(Work, Mode) -> Cadence| submits_primary(|fake| {
        fake.intervals.push((TimingWord(0x55), FrameInterval(interval)));
        fake.cadence = cadence;
    });
    assert!(paced(33_333, fast));
    assert!(!paced(40_000, fast));
    assert!(paced(40_000, slow));
    assert!(submits_primary(|fake| fake.cadence = slow));
}

#[test]
fn record_and_profile_selection_take_precedence_over_the_frame_rate() {
    assert!(submits_primary(|fake| { fake.records.profile.selection = ProfileSelection::Record; fake.cadence = by_record; }));
    assert!(submits_primary(|fake| { fake.records.profile.profile_cadence = true; fake.cadence = by_profile; }));
    assert!(!submits_primary(|fake| { fake.records.profile.profile_cadence = true; fake.cadence = by_record; }));
}
