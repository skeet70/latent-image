use crate::boundary::{Adjustment, ClientRef, Frame, LensReading, Setting, Trace};
use crate::capture_boundary::{CalculationResult, CaptureAdjustment, MoveProfile, RecordField, SlotField};
use crate::ports::{CalculationsJob, HalfPressMode};
use crate::preparation::{HalfPressMask, HalfPressParameters};
use crate::shot_ports_fakes::{Event, ExternalFake, Memory};
use crate::state::StateMemory;
use crate::stock::{self as layout, state};
use crate::tests::record;
use crate::{Outcome, Shot, ShotPorts};
use std::vec::Vec;
use xm1_saclac_controllers::children::{ShotControl, ShotEvent, ShotTarget};
use xm1_saclac_interface::capture_results::{FirstResult, SecondResult, ThirdResult, FourthResult};
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion};

type Ports = ShotPorts<Memory, ExternalFake>;

fn start(shot: &mut Shot, ports: &mut Ports, control: ShotControl) -> Outcome {
    shot.handle(ShotEvent::Control { control, record: record() }, ports)
}

fn complete(shot: &mut Shot, ports: &mut Ports, target: ShotTarget, completion: Completion, payload: &[u8]) -> Outcome {
    let completed = Completed { completion, parameters: stock::parameters(payload) };
    shot.handle(ShotEvent::Completed { target, completed, record: record() }, ports)
}

fn replies(ports: &mut Ports) -> Vec<(CommandKind, i32)> {
    ports.external().events.iter().filter_map(|event| match event {
        Event::Reply(reply) => { assert_eq!(reply.record, Some(record())); Some((reply.command, reply.status.0)) }
        _ => None,
    }).collect()
}

#[test]
fn a_half_press_requests_a_calculation_and_replies_only_after_the_lens_and_commit() {
    let mut external = ExternalFake::new();
    external.half_press = HalfPressParameters { mode: HalfPressMode(6), mask: HalfPressMask(10), ..HalfPressParameters::default() };
    let mut ports = Ports::new(Memory::default(), external);
    let mut shot = Shot::new();
    assert_eq!(start(&mut shot, &mut ports, ShotControl::StartS1), Outcome::Pending);
    assert_eq!(ports.external().events, [Event::Trace(Trace::Prepare), Event::Trace(Trace::ExposureBegun), Event::Trace(Trace::CorrectionRequested), Event::Correction]);

    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::HalfPress, Completion::Calculation8f, &[0; 156]), Outcome::Pending);
    assert_eq!(replies(&mut ports), [(CommandKind::HalfPress, 3)]);
    assert_eq!(ports.external().events.last(), Some(&Event::Calculation(Completion::Calculation90)));
    assert_eq!(ports.state().memory().word(state::BLOCK), stock::record_handle_word(record()));
    ports.state().memory().write(state::RESULT_WORD, &0x345u32.to_le_bytes());
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::HalfPress, Completion::Calculation90, &[]), Outcome::Pending);
    assert!(ports.external().events.iter().any(|event| matches!(event, Event::Absolute(_, MoveProfile::Finish { .. }, Completion::Lens3a))));
    assert!(ports.external().events.iter().any(|event| matches!(event, Event::Relative(_, _, MoveProfile::Finish { .. }, Completion::Lens35))));
    assert_eq!(ports.state().memory().word(state::LENS_NOTICE), 1);

    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::HalfPress, Completion::Lens35, &[]), Outcome::Pending);
    assert_eq!(ports.external().events.last(), Some(&Event::Commit));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::HalfPress, Completion::Lens3a, &[]), Outcome::Pending);
    assert!(!replies(&mut ports).contains(&(CommandKind::HalfPress, 0)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::HalfPress, Completion::Image53, &[]), Outcome::Complete);
    assert_eq!(replies(&mut ports), [(CommandKind::HalfPress, 3), (CommandKind::HalfPress, 4), (CommandKind::HalfPress, 6), (CommandKind::HalfPress, 0)]);
    assert_eq!(ports.external().events.last(), Some(&Event::Trace(Trace::HalfPressReplied)));
    assert!(!ports.external().events.contains(&Event::Fault));
}

#[test]
fn a_capture_measures_then_runs_all_four_jobs_and_delivers_their_results_before_finishing() {
    let mut external = ExternalFake::new();
    external.capture.set_adjustment(CaptureAdjustment::Diagnostics, 1);
    external.capture.set_adjustment(CaptureAdjustment::SecondaryStyle, 1);
    external.capture.set_slot(0, SlotField::ProfileSelector, 8);
    let mut ports = Ports::new(Memory::default(), external);
    let mut shot = Shot::new();
    assert_eq!(start(&mut shot, &mut ports, ShotControl::InitializeCapture), Outcome::Pending);
    assert_eq!(ports.external().events, [Event::Setup(false, Completion::Statistics83)]);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Statistics83, &[]), Outcome::Pending);
    assert!(ports.external().events.iter().any(|event| matches!(event, Event::Measure(_, [Completion::Image5b, Completion::Image5c]))));
    assert_eq!(replies(&mut ports), [(CommandKind::Command07, 7)]);
    assert!(!ports.external().events.iter().any(|event| matches!(event, Event::Capture(..))));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Image5c, &[0, 0, 0, 0, 3, 0, 0, 0]), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Image5b, &[]), Outcome::Pending);
    let capture = ports.external().events.iter().find_map(|event| if let Event::Capture(request, completions) = event { Some((*request, *completions)) } else { None }).unwrap();
    assert_eq!((capture.1.outputs, capture.1.readout, capture.1.delivery), (true, true, false));

    let readout = [6, 2, 0, 0, 3, 0, 0, 0, 0x54, 0x32, 0, 0];
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Image61, &readout), Outcome::Pending);
    assert_eq!(ports.external().events.last(), Some(&Event::Job(CalculationsJob::First)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Calculation99, &[1; 12]), Outcome::Pending);
    assert!(matches!(ports.external().events.last(), Some(Event::SecondJob(_))));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Calculation97, &[2; 48]), Outcome::Pending);
    assert_eq!(ports.external().events.last(), Some(&Event::Job(CalculationsJob::Third)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Calculation98, &0x1234u32.to_le_bytes()), Outcome::Pending);
    assert_eq!(ports.external().events.last(), Some(&Event::Job(CalculationsJob::Fourth)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Calculation9a, &[3, 0, 4, 0, 5, 0]), Outcome::Pending);
    let results: Vec<_> = ports.external().events.iter().filter(|event| matches!(event, Event::First(_) | Event::Second(_) | Event::Third(_) | Event::Fourth(_) | Event::Published(_))).cloned().collect();
    assert_eq!(results, [
        Event::First(FirstResult([1; 12])), Event::Published(CalculationsJob::First),
        Event::Second(SecondResult([2; 48])), Event::Published(CalculationsJob::Second),
        Event::Third(ThirdResult(0x1234)), Event::Published(CalculationsJob::Third),
        Event::Fourth(FourthResult { first: 3, second: 4, third: 5 }), Event::Published(CalculationsJob::Fourth),
    ]);
    assert!(!replies(&mut ports).contains(&(CommandKind::Command07, 0)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Image60, &[]), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Image5e, &[]), Outcome::Complete);
    assert_eq!(replies(&mut ports), [(CommandKind::Command07, 7), (CommandKind::Command07, 10), (CommandKind::Command07, 0)]);
    assert!(!ports.external().events.contains(&Event::Fault));
}

#[test]
fn a_capture_returns_the_lens_before_it_starts_the_primary_job() {
    let mut external = ExternalFake::new();
    external.capture.set(RecordField::ResourceKind, 2);
    external.capture.pairs[4].reference = 10;
    external.capture.slot_references.insert(0x1000, 42);
    external.basic.set_lens(LensReading::Reference, 20);
    external.basic.set_lens(LensReading::State, 10);
    external.basic.set_lens(LensReading::Position, 4);
    external.basic.set_lens(LensReading::Kind, 3);
    let mut ports = Ports::new(Memory::default(), external);
    let mut shot = Shot::new();
    assert_eq!(start(&mut shot, &mut ports, ShotControl::InitializeCapture), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Statistics83, &[]), Outcome::Pending);
    assert!(ports.state().return_active());
    assert!(matches!(ports.external().events.last(), Some(Event::Reference(_, CalculationResult::LensReturn, Completion::Calculation90))));
    assert!(replies(&mut ports).is_empty());
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Calculation90, &[]), Outcome::Pending);
    assert!(ports.external().events.contains(&Event::Absolute(42, MoveProfile::LensReturn, Completion::Lens3a)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Lens3a, &[]), Outcome::Pending);
    assert!(ports.external().events.contains(&Event::Relative(132, 3, MoveProfile::LensReturn, Completion::Lens35)));
    assert!(!ports.external().events.iter().any(|event| matches!(event, Event::Capture(..))));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Lens35, &[]), Outcome::Pending);
    assert!(!ports.state().return_active());
    assert!(ports.external().events.iter().any(|event| matches!(event, Event::Capture(..))));
    assert!(!ports.external().events.contains(&Event::Fault));
}

#[test]
fn the_half_press_statistics_and_capture_selection_map_share_the_frame_tag() {
    let mut external = ExternalFake::new();
    external.half_press.frame_slot = Some(ClientRef(0x300));
    external.basic.frame = Some(Frame(0x8000));
    external.basic.set(Setting::DumpArgument, 1);
    external.basic.set_adjustment(Adjustment::StatisticsDump, 0x80);
    external.capture.frame = Frame(0x8000);
    external.capture.set_adjustment(CaptureAdjustment::Diagnostics, 0x80);
    external.capture.set(RecordField::MapPresent, 1);
    let mut ports = Ports::new(Memory::default(), external);
    let mut shot = Shot::new();
    assert_eq!(start(&mut shot, &mut ports, ShotControl::StartS1), Outcome::Complete);
    assert!(ports.external().events.contains(&Event::DumpHeader(Frame(0x8000))));
    assert_eq!(start(&mut shot, &mut ports, ShotControl::InitializeCapture), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Statistics83, &[]), Outcome::Pending);
    assert!(!ports.external().capture.calls.iter().any(|call| matches!(call, crate::capture_fakes::Call::PrepareDumpArea(..))));
    let tags: Vec<_> = ports.external().events.iter().filter(|event| matches!(event, Event::FrameWord(..))).cloned().collect();
    assert_eq!(tags, [
        Event::FrameWord(Frame(0x8000), layout::statistics_dump::DUMP, layout::statistics_dump::START_TAG),
        Event::FrameWord(Frame(0x8000), layout::statistics_dump::END_TAG_OFFSET, layout::statistics_dump::END_TAG),
    ]);
}

#[test]
fn an_idle_capture_setting_command_commits_the_delivered_result_before_replying() {
    let mut ports = Ports::new(Memory::default(), ExternalFake::new());
    let mut shot = Shot::new();
    let command = Command { kind: CommandKind::Command08, parameters: stock::parameters(&[]) };
    assert_eq!(shot.handle(ShotEvent::Command { target: ShotTarget::Capture, command, record: record() }, &mut ports), Outcome::Pending);
    let payload: Vec<_> = [0u32, 2, 3, 5, 7, 11].into_iter().flat_map(u32::to_le_bytes).collect();
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Capture, Completion::Statistics85, &payload), Outcome::Complete);
    assert!(ports.external().capture.calls.contains(&crate::capture_fakes::Call::StoreSettingResult(record(), crate::capture_boundary::SettingResult([2, 3, 5, 7, 11]))));
    assert_eq!(replies(&mut ports), [(CommandKind::Command08, 0)]);
}

#[test]
fn a_live_sequence_uses_the_kept_correction_on_a_null_callback_and_requests_its_calculation() {
    let mut ports = Ports::new(Memory::default(), ExternalFake::new());
    let mut correction = xm1_saclac_interface::exposure::Correction::default();
    correction.0[3] = 50;
    ports.keep_correction(&correction);
    let mut shot = Shot::new();
    assert_eq!(start(&mut shot, &mut ports, ShotControl::StartLiveSequence), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Live, Completion::Calculation8f, &[]), Outcome::Pending);
    assert!(ports.external().basic.exposure.calls.contains(&crate::exposure_fakes::ExposureCall::StoreExposure(0, 127)));
    assert_eq!(ports.external().events.last(), Some(&Event::Calculation(Completion::Calculation90)));
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Live, Completion::Calculation90, &[]), Outcome::Pending);
    assert_eq!(complete(&mut shot, &mut ports, ShotTarget::Live, Completion::Lens3a, &[]), Outcome::Complete);
    assert!(replies(&mut ports).is_empty());
    assert_eq!(ports.external().basic.events.last(), Some(&crate::leaf_fakes::Event::Hold(false)));
    assert!(!ports.external().events.contains(&Event::Fault));
}
