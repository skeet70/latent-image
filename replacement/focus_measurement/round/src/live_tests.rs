use crate::acquisition::{self, Pass};
use crate::fakes::{Outbound, Ports};
use crate::image::{ImageChannel, ImageEffect, ImageRequest, ReplyValue};
use crate::jobs::{self, Calculation};
use crate::lanes::Selection;
use crate::live::{Effects, Measurement};
use crate::planner::Mode;
use crate::record::Statistics;
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
    Finished { kind: ImageRequest, failed: bool, channel: ImageChannel, value: ReplyValue },
    Fault,
}

struct Live { traffic: Vec<Traffic>, source_ready: bool, failed: bool }

impl Live {
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

type Fake = Ports<Live>;

fn fake(available: Selection) -> Fake { Ports::new(Some(available), Live { traffic: Vec::new(), source_ready: false, failed: false }) }

impl Effects for Fake {
    fn planning_mode(&mut self, _: RecordHandle) -> Mode { Mode::Default }
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
    fn acquisition(&mut self, _: RecordHandle, effect: acquisition::Effect) { self.extra.traffic.push(Traffic::Acquisition(effect)); }
    fn selection_changed(&mut self, _: RecordHandle) -> bool { false }
    fn image(&mut self, _: RecordHandle, effect: ImageEffect<'_>) {
        self.extra.traffic.push(match effect {
            ImageEffect::Initialize => Traffic::Image("initialize"),
            ImageEffect::PrepareGeometry => Traffic::Image("geometry"),
            ImageEffect::Request { .. } => Traffic::Image("request"),
            ImageEffect::Cancel(_) => Traffic::Image("cancel"),
            ImageEffect::Stop => Traffic::Image("stop"),
            ImageEffect::Interrupted { .. } => Traffic::Image("interrupted"),
            ImageEffect::Finished { kind, failed, channel, value, .. } => Traffic::Finished { kind, failed, channel, value },
        });
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
        Traffic::Calculation(Calculation::Window, jobs::Effect::Initialize),
        Traffic::Calculation(Calculation::Window, jobs::Effect::Enable),
        Traffic::Calculation(Calculation::Source, jobs::Effect::Initialize),
    ]);
}

#[test]
fn a_round_submits_window_and_primary_acquisition_and_collects_their_results() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [
        Traffic::Calculation(Calculation::Window, jobs::Effect::Submit),
        Traffic::Acquisition(acquisition::Effect::SubmitPrimary(Pass::First)),
    ]);
    assert_eq!(measurement.handle(completion(Completion::Calculation8d), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [Traffic::Calculation(Calculation::Window, jobs::Effect::Collect)]);
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
    assert_eq!(fake.extra.take(), [
        Traffic::Acquisition(acquisition::Effect::Release),
        Traffic::Search("stop"),
        Traffic::Calculation(Calculation::Window, jobs::Effect::Release),
    ]);
}

#[test]
fn an_image_command_replies_through_the_image_lane() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { image: true, ..ACQUISITION });
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    let mut words = [0u8; 20];
    words[12..16].copy_from_slice(&5u32.to_le_bytes());
    words[16..20].copy_from_slice(&9u32.to_le_bytes());
    let image = ChildEvent::Command { command: Command { kind: CommandKind::Command14, parameters: stock::parameters(&words) }, record: record() };
    assert_eq!(measurement.handle(image, &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.extra.take(), [Traffic::Image("geometry"), Traffic::Image("request")]);
    fake.extra.failed = true;
    measurement.handle(completion(Completion::Image47), &mut fake);
    assert_eq!(fake.extra.take(), [Traffic::Finished { kind: ImageRequest::Image, failed: true, channel: ImageChannel(5), value: ReplyValue(9) }]);
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
    assert_eq!(fake.take(), [Outbound::Capture(Statistics::Primary), Outbound::MeasurementHistory]);
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
        Outbound::Reply { target, callback: stock::optional_callback(0xf0a0).unwrap(), status: CompletionStatus(0) },
    ]);
}
