use crate::acquisition::Branch;
use crate::fakes::{AcquisitionTraffic, Outbound, Ports, Published};
use crate::image::{ChannelCount, LensReport};
use crate::lanes::Selection;
use crate::live::{Effects, Measurement};
use crate::pipeline::{Statistics, TimingWord};
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{FrameInterval, FrameRate, ProfileSelection, RetainedWord, WindowPosition, WindowStatus};
use crate::record::Request;
use crate::source::{ProcessingProfile, SegmentSettings, SourceProfile, SourceStrength};
use crate::window::WindowResult;
use crate::round::{LaneSignal, LaneStatus};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl};
use xm1_saclac_interface::round_image::{ImageArea, ImageArguments, ImageChannel, ImageLaneRequest};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Search(&'static str),
    Fault,
}

struct Live { traffic: Vec<Traffic> }

impl Live {
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

type Fake = Ports<Live>;

fn fake(available: Selection) -> Fake { Ports::new(Some(available), Live { traffic: Vec::new() }) }

fn acquisition_requests(outbound: &[Outbound]) -> usize {
    outbound.iter().filter(|traffic| matches!(traffic, Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::Live, _)))).count()
}

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
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }
fn control(control: MeasurementControl) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}
fn completion(completion: Completion) -> ChildEvent<'static, MeasurementControl> { finished(completion, &[]) }
fn finished(completion: Completion, parameters: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(parameters) }, record: record() }
}

fn command(words: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Command { command: Command { kind: CommandKind::Command14, parameters: stock::parameters(words) }, record: record() }
}

const ACQUISITION: Selection = Selection { acquisition: true, search: false, auxiliary: false, image: false, source: false };

#[test]
fn initialization_enables_acquisition_and_window_and_initializes_the_other_lanes() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.image_client = Some(StockCallbackWord(0x00e0_1234));
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [Traffic::Search("initialize")]);
    assert_eq!(fake.acquisition.word & 1, 1);
    let outbound = fake.take();
    assert!(outbound.contains(&Outbound::NotifyImageClient(StockCallbackWord(0x00e0_1234))));
    assert!(outbound.iter().any(|traffic| matches!(traffic, Outbound::Acquisition(AcquisitionTraffic::Exposure(_)))));
    assert!(fake.records.in_use.primary && fake.records.in_use.search);
    assert!(fake.auxiliary.publication.published);
    assert_eq!((fake.source.published, fake.source.callbacks_enabled), (Some(Published::Withdrawn), Some(false)));
}

#[test]
fn a_round_offering_the_source_reads_it_after_the_window_and_publishes_the_average() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { source: true, ..ACQUISITION });
    fake.adjustment.controllers = RetainedWord(1);
    fake.source.profile = SourceProfile {
        measures_source: true,
        processing: ProcessingProfile { selection: ProfileSelection::Record, ..ProcessingProfile::default() },
        ..SourceProfile::default()
    };
    fake.source.settings.segments = [SegmentSettings { ring_length: 1, newest_weight: 0, gain: 128 }; 4];
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    assert!(matches!(fake.source.published, Some(Published::Fresh { .. })));
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    fake.take();
    measurement.handle(completion(Completion::Calculation8d), &mut fake);
    assert!(fake.take().iter().any(|traffic| matches!(traffic, Outbound::Request(Request::Source(_)))));
    let reading = [7, 0, 0, 0, 0, 0];
    let completed = Completed { completion: Completion::Calculation9a, parameters: stock::parameters(&reading) };
    measurement.handle(ChildEvent::Completed { completed, record: record() }, &mut fake);
    assert!(matches!(fake.source.published, Some(Published::Averaged { strength: SourceStrength(7), .. })));
}

#[test]
fn a_round_submits_window_and_primary_acquisition_and_collects_their_results() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
    let outbound = fake.take();
    assert_eq!(acquisition_requests(&outbound), 1);
    assert!(outbound.iter().any(|traffic| matches!(traffic, Outbound::Request(Request::Window(_)))));
    fake.window_result = Some(WindowResult { status: WindowStatus(5), position: WindowPosition([1, 2]) });
    assert_eq!(measurement.handle(completion(Completion::Calculation8d), &mut fake), ChildOutcome::Pending);
    assert_eq!((fake.records.window.status, fake.records.window_position), (WindowStatus(5), WindowPosition([1, 2])));
    let result = xm1_saclac_interface::calc::SharedAddress(0x0169_0000);
    fake.acquisition.acquisition_result = Some(xm1_saclac_interface::round_acquisition::AcquisitionResult { address: result, ..Default::default() });
    assert_eq!(measurement.handle(completion(Completion::Calculation87), &mut fake), ChildOutcome::Pending);
    assert!(fake.take().contains(&Outbound::Acquisition(AcquisitionTraffic::Released(result))));
    assert_eq!(fake.acquisition.block.collected, 1);
}

#[test]
fn stopping_a_round_releases_each_enabled_job() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    fake.take();
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [Traffic::Search("stop")]);
    assert_eq!(fake.take(), [Outbound::Acquisition(AcquisitionTraffic::Statistics)]);
}

#[test]
fn an_image_command_asks_imgdc_and_replies_with_its_status() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { image: true, ..ACQUISITION });
    fake.channel_count = ChannelCount(8);
    fake.lens = LensReport { first: true, second: false, third: true };
    fake.image_area = |channel, area| ImageArea([area.0[0] + channel.0 as i16, -1]);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    fake.take();
    let image = |channel: u32| -> Vec<u8> { [0xd00, 0x11, 0x22, channel, 9].iter().flat_map(|word: &u32| word.to_le_bytes()).collect() };
    let target = ReplyTarget::Command(CommandKind::Command14);
    let callback = stock::optional_callback(0xd00).unwrap();

    let words = image(5);
    assert_eq!(measurement.handle(command(&words), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [
        Outbound::PrepareImageArea(ImageChannel(5), ImageArea([0, 0])),
        Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([5, -1]), channel: ImageChannel(5) }),
    ]);
    measurement.handle(finished(Completion::Image47, &[1, 0, 0, 0, 0, 0, 0, 0]), &mut fake);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback, status: CompletionStatus(-94), values: [5, 9, 6] },
    ]);

    let words = image(8);
    measurement.handle(command(&words), &mut fake);
    assert_eq!(fake.take()[1], Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([13, -1]), channel: ImageChannel(8) }));
    measurement.handle(finished(Completion::Image47, &[0; 8]), &mut fake);
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
    assert!(!measurement.history().started());
    fake.take();
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    let traffic = fake.take();
    assert_eq!(traffic[..3], [Outbound::Capture(Statistics::Primary), Outbound::Capture(Statistics::Search), Outbound::Capture(Statistics::History)]);
    assert!(matches!(traffic[3], Outbound::Request(Request::Window(_))));
    assert!(measurement.history().newest().written());
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
    fake.acquisition.view.start = 1;
    fake.acquisition.range.timing = TimingWord(0x55);
    fake.set_current_interval(Some(FrameInterval(1_000)));
    configure(&mut fake);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    acquisition_requests(&fake.take()) == 1
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
