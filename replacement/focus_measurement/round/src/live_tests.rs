use crate::acquisition::{self, Pass};
use crate::image::{ImageChannel, ImageEffect, ImageRequest, ReplyValue};
use crate::jobs::{self, Calculation};
use crate::lanes::Selection;
use crate::live::{Effects, Measurement};
use crate::planner::{Cadence, Mode, Work};
use crate::round::{LaneSignal, LaneStatus, RecordEffect};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl};
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, RecordHandle};
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

struct Fake { traffic: Vec<Traffic>, available: Option<Selection>, source_ready: bool, failed: bool }

impl Fake {
    fn new(available: Selection) -> Self { Self { traffic: Vec::new(), available: Some(available), source_ready: false, failed: false } }
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

impl Effects for Fake {
    fn available(&mut self, _: RecordHandle) -> Option<Selection> { self.available }
    fn requested_changes(&mut self) -> (Selection, Selection) { (Selection::default(), Selection::default()) }
    fn search_permitted(&mut self) -> bool { true }
    fn alternate_search(&mut self, _: RecordHandle) -> bool { false }
    fn autofocus_geometry(&mut self, _: RecordHandle) -> bool { false }
    fn planning_mode(&mut self, _: RecordHandle) -> Mode { Mode::Default }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
    fn record_effect(&mut self, _: RecordHandle, _: RecordEffect) {}
    fn fault(&mut self) { self.traffic.push(Traffic::Fault); }
    fn search(&mut self, signal: LaneSignal<'_>, _: RecordHandle) -> LaneStatus {
        let (name, status) = match signal {
            LaneSignal::Work(_) => ("work", LaneStatus::Pending),
            LaneSignal::Initialize => ("initialize", LaneStatus::Complete),
            LaneSignal::Enable => ("enable", LaneStatus::Complete),
            LaneSignal::Stop => ("stop", LaneStatus::Complete),
            _ => ("other", LaneStatus::Complete),
        };
        self.traffic.push(Traffic::Search(name));
        status
    }
    fn calculation(&mut self, _: RecordHandle, calculation: Calculation, effect: jobs::Effect) { self.traffic.push(Traffic::Calculation(calculation, effect)); }
    fn source_available(&mut self, _: RecordHandle) -> bool { self.source_ready }
    fn result_changed(&mut self, _: RecordHandle, _: Calculation) -> bool { false }
    fn acquisition(&mut self, _: RecordHandle, effect: acquisition::Effect) { self.traffic.push(Traffic::Acquisition(effect)); }
    fn selection_changed(&mut self, _: RecordHandle) -> bool { false }
    fn image(&mut self, _: RecordHandle, effect: ImageEffect<'_>) {
        self.traffic.push(match effect {
            ImageEffect::Initialize => Traffic::Image("initialize"),
            ImageEffect::PrepareGeometry => Traffic::Image("geometry"),
            ImageEffect::Request { .. } => Traffic::Image("request"),
            ImageEffect::Cancel(_) => Traffic::Image("cancel"),
            ImageEffect::Stop => Traffic::Image("stop"),
            ImageEffect::Interrupted { .. } => Traffic::Image("interrupted"),
            ImageEffect::Finished { kind, failed, channel, value, .. } => Traffic::Finished { kind, failed, channel, value },
        });
    }
    fn image_failed(&mut self, _: Completed<'_>) -> bool { self.failed }
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
    let mut fake = Fake::new(ACQUISITION);
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.take(), [
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
    let mut fake = Fake::new(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [
        Traffic::Calculation(Calculation::Window, jobs::Effect::Submit),
        Traffic::Acquisition(acquisition::Effect::SubmitPrimary(Pass::First)),
    ]);
    assert_eq!(measurement.handle(completion(Completion::Calculation8d), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [Traffic::Calculation(Calculation::Window, jobs::Effect::Collect)]);
    assert_eq!(measurement.handle(completion(Completion::Calculation87), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [Traffic::Acquisition(acquisition::Effect::CollectPrimary), Traffic::Acquisition(acquisition::Effect::PublishPrimary)]);
}

#[test]
fn stopping_a_round_releases_each_enabled_job() {
    let mut measurement = Measurement::new();
    let mut fake = Fake::new(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.take(), [
        Traffic::Acquisition(acquisition::Effect::Release),
        Traffic::Search("stop"),
        Traffic::Calculation(Calculation::Window, jobs::Effect::Release),
    ]);
}

#[test]
fn an_image_command_replies_through_the_image_lane() {
    let mut measurement = Measurement::new();
    let mut fake = Fake::new(Selection { image: true, ..ACQUISITION });
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    let mut words = [0u8; 20];
    words[12..16].copy_from_slice(&5u32.to_le_bytes());
    words[16..20].copy_from_slice(&9u32.to_le_bytes());
    let image = ChildEvent::Command { command: Command { kind: CommandKind::Command14, parameters: stock::parameters(&words) }, record: record() };
    assert_eq!(measurement.handle(image, &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [Traffic::Image("geometry"), Traffic::Image("request")]);
    fake.failed = true;
    measurement.handle(completion(Completion::Image47), &mut fake);
    assert_eq!(fake.take(), [Traffic::Finished { kind: ImageRequest::Image, failed: true, channel: ImageChannel(5), value: ReplyValue(9) }]);
}
