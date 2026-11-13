use crate::fakes::{Outbound, Ports};
use crate::jobs::Effect;
use crate::pipeline::{GainStep, PipelineSnapshot, PipelineWord, TimingWord};
use crate::profile::{ProfileMode, ProfileSelection, RetainedHalfword, RetainedWord, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus};
use crate::record::{AdjustmentWords, Request, TraceMark};
use crate::window::{Window, WindowRequest, WindowResult};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

fn completed() -> Completed<'static> { Completed { completion: Completion::Calculation8d, parameters: stock::parameters(&[]) } }

fn ports() -> Ports {
    let mut ports = Ports::new(None, ());
    ports.snapshot = PipelineSnapshot {
        timing: TimingWord(3),
        flags: 9,
        gain_step: GainStep(2),
        exposure: [PipelineWord(40), PipelineWord(41)],
        levels: [500, 300],
        adjustment: PipelineWord(0),
        ..PipelineSnapshot::default()
    };
    ports.gain_offsets[2] = 20;
    ports.exposure_value = -7;
    ports.records.window = WindowRecord { status: WindowStatus(2), first: RetainedWord(11), halfword: RetainedHalfword(12), second: RetainedWord(13) };
    ports
}

fn submitted(ports: &mut Ports) -> WindowRequest {
    ports.take().into_iter().find_map(|traffic| match traffic {
        Outbound::Request(Request::Window(request)) => Some(request),
        _ => None,
    }).expect("a window request")
}

#[test]
fn enabling_keeps_the_primary_statistics_in_use() {
    let mut ports = ports();
    ports.records.in_use = StatisticsInUse { primary: false, search: true, auxiliary: false };
    Window::new().effect(Effect::Enable, record(), None, &mut ports);
    assert_eq!(ports.records.in_use, StatisticsInUse { primary: true, search: true, auxiliary: false });
}

#[test]
fn a_request_carries_the_adjusted_levels_and_the_window_record() {
    let mut ports = ports();
    ports.records.profile.selection = ProfileSelection::Other(2);
    ports.window_refresh = true;
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    let request = submitted(&mut ports);
    assert_eq!(request.reference, 280);
    assert_eq!(request.level, 500 - 280 - 7);
    assert_eq!(request.record, ports.records.window);
    assert!(request.refresh);
    assert!(!request.selected);
    assert!(!ports.window_refresh);

    ports.snapshot.adjustment = PipelineWord(1);
    ports.records.profile.mode = ProfileMode(4);
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    let request = submitted(&mut ports);
    assert_eq!(request.level, 500 - 280 - 7 - 100);
    assert!(!request.refresh);
    assert!(request.selected);
}

#[test]
fn a_request_carries_the_controllers_word_and_the_search_word_only_for_a_profile_that_takes_it() {
    let mut ports = ports();
    ports.adjustment = AdjustmentWords { controllers: RetainedWord(1), search: RetainedWord(1) };
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    let request = submitted(&mut ports);
    assert_eq!(request.adjustment, AdjustmentWords { controllers: RetainedWord(1), search: RetainedWord(0) });
    assert!(!request.selected);

    ports.records.profile.takes_search_adjustment = true;
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    let request = submitted(&mut ports);
    assert_eq!(request.adjustment, ports.adjustment);
    assert!(request.selected);

    ports.adjustment.search = RetainedWord(2);
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    assert!(!submitted(&mut ports).selected);
}

#[test]
fn the_levels_wrap_as_halfwords() {
    let mut ports = ports();
    ports.snapshot.levels = [10, 0];
    ports.exposure_value = 0;
    ports.snapshot.adjustment = PipelineWord(1);
    Window::new().effect(Effect::Submit, record(), None, &mut ports);
    let request = submitted(&mut ports);
    assert_eq!(request.reference, 0u16.wrapping_sub(20));
    assert_eq!(request.level, 10u16.wrapping_add(20).wrapping_sub(100));
}

#[test]
fn a_status_that_holds_power_requests_it_once_before_the_traced_request() {
    let mut ports = ports();
    ports.traces = true;
    let mut window = Window::new();
    window.effect(Effect::Submit, record(), None, &mut ports);
    let traffic = ports.take();
    assert_eq!(traffic[..2], [Outbound::Power(true), Outbound::Trace(TraceMark::WindowRequest)]);
    assert!(matches!(traffic[2], Outbound::Request(Request::Window(_))));
    window.effect(Effect::Submit, record(), None, &mut ports);
    assert!(!ports.take().contains(&Outbound::Power(true)));

    ports.records.window.status = WindowStatus(3);
    let mut idle = Window::new();
    idle.effect(Effect::Submit, record(), None, &mut ports);
    assert!(!ports.take().contains(&Outbound::Power(true)));
}

#[test]
fn a_result_is_stored_and_a_status_outside_one_and_two_releases_the_power() {
    let mut ports = ports();
    let mut window = Window::new();
    window.effect(Effect::Submit, record(), None, &mut ports);
    ports.take();
    ports.window_result = Some(WindowResult { status: WindowStatus(1), position: WindowPosition([4, 5]) });
    window.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert!(ports.take().is_empty());
    assert_eq!((ports.records.window.status, ports.records.window_position), (WindowStatus(1), WindowPosition([4, 5])));

    ports.window_result = Some(WindowResult { status: WindowStatus(0), position: WindowPosition([6, 7]) });
    window.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert_eq!(ports.take(), [Outbound::Power(false)]);
    assert_eq!((ports.records.window.status, ports.records.window_position), (WindowStatus(0), WindowPosition([6, 7])));
    window.effect(Effect::Release, record(), None, &mut ports);
    assert!(ports.take().is_empty());
}

#[test]
fn a_completion_without_a_result_changes_nothing() {
    let mut ports = ports();
    let mut window = Window::new();
    window.effect(Effect::Submit, record(), None, &mut ports);
    ports.take();
    window.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert!(ports.take().is_empty());
    assert_eq!(ports.records.window.status, WindowStatus(2));
    window.effect(Effect::Release, record(), None, &mut ports);
    assert_eq!(ports.take(), [Outbound::Power(false)]);
}

#[test]
fn initialization_forgets_held_power_without_releasing_it() {
    let mut ports = ports();
    let mut window = Window::new();
    window.effect(Effect::Submit, record(), None, &mut ports);
    window.effect(Effect::Initialize, record(), None, &mut ports);
    ports.take();
    window.effect(Effect::Release, record(), None, &mut ports);
    assert!(ports.take().is_empty());
}
