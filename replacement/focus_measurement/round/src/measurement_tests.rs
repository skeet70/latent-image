use crate::measurement::{Measurement, Ports, RecordEffect, RoundControl, RoundOutcome, SearchNotification};
use crate::scheduling::{Lane, Lanes};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementOutcome, MeasurementUpdate, Restart};
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, RecordHandle};
use std::{collections::VecDeque, vec::Vec};

struct Backend {
    controls: VecDeque<RoundOutcome>,
    events: VecDeque<RoundOutcome>,
    enabled: Lanes,
    enabling: Lanes,
    draining: Lanes,
    publications: Vec<(RecordHandle, RecordEffect)>,
    records: Vec<RecordHandle>,
    notifications: Vec<SearchNotification>,
    alternate: bool,
    faults: usize,
}
impl Backend {
    fn measuring() -> Self {
        let mut enabled = Lanes::empty(); enabled.insert(Lane::Acquisition);
        Self { controls: [RoundOutcome::Pending].into(), events: VecDeque::new(), enabled, enabling: Lanes::empty(), draining: Lanes::empty(), publications: Vec::new(), records: Vec::new(), notifications: Vec::new(), alternate: false, faults: 0 }
    }
}
impl Ports for Backend {
    fn refresh_selection(&mut self, record: RecordHandle) { self.records.push(record); }
    fn round_control(&mut self, record: RecordHandle, _: RoundControl) -> RoundOutcome { self.records.push(record); self.controls.pop_front().expect("specified round outcome") }
    fn round_event(&mut self, event: ChildEvent<'_, MeasurementControl>) -> RoundOutcome {
        self.records.push(match event { ChildEvent::Control { record, .. } | ChildEvent::Command { record, .. } | ChildEvent::Completed { record, .. } => record });
        self.events.pop_front().expect("specified event outcome")
    }
    fn record_effect(&mut self, record: RecordHandle, effect: RecordEffect) { self.publications.push((record, effect)); }
    fn enabled(&self) -> Lanes { self.enabled }
    fn enabling(&self) -> Lanes { self.enabling }
    fn draining(&self) -> Lanes { self.draining }
    fn allow_search(&mut self, _: bool) {}
    fn search_is_alternate(&self, _: RecordHandle) -> bool { self.alternate }
    fn geometry_changed(&self, _: RecordHandle) -> bool { true }
    fn notify_search(&mut self, _: RecordHandle, notification: SearchNotification) { self.notifications.push(notification); }
    fn fault(&mut self) { self.faults += 1; }
}
fn control(control: MeasurementControl) -> ChildEvent<'static, MeasurementControl> { ChildEvent::Control { control, record: stock::record_handle_from_a2(123), parameters: stock::parameters(&[]) } }
fn command(kind: CommandKind) -> ChildEvent<'static, MeasurementControl> { ChildEvent::Command { record: stock::record_handle_from_a2(123), command: Command { kind, parameters: stock::parameters(&[]) } } }
fn completion(completion: Completion) -> ChildEvent<'static, MeasurementControl> { ChildEvent::Completed { record: stock::record_handle_from_a2(456), completed: Completed { completion, parameters: stock::parameters(&[]) } } }

#[test]
fn measurement_initialization_accepts_a_pending_round_and_rejects_double_start() {
    let mut measurement = Measurement::new();
    let mut backend = Backend::measuring();
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut backend), ChildOutcome::Complete);
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut backend), ChildOutcome::Fault);
    assert_eq!(backend.faults, 1);
}

#[test]
fn stop_drains_a_pending_image_with_its_completion_record() {
    let mut measurement = Measurement::new();
    let mut backend = Backend::measuring();
    measurement.handle(control(MeasurementControl::Initialize), &mut backend);
    backend.controls.push_back(RoundOutcome::Pending);
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut backend), ChildOutcome::Pending);
    backend.events.push_back(RoundOutcome::Complete);
    assert_eq!(measurement.handle(completion(Completion::Image47), &mut backend), ChildOutcome::Complete);
    assert_eq!(backend.records.last(), Some(&stock::record_handle_from_a2(456)));
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut backend), ChildOutcome::Complete);
}

#[test]
fn refresh_geometry_is_pending_without_initializing_measurement() {
    let mut measurement = Measurement::new();
    let mut backend = Backend::measuring();
    assert_eq!(measurement.handle(control(MeasurementControl::Reconfigure), &mut backend), ChildOutcome::Pending);
    assert_eq!(backend.publications, [(stock::record_handle_from_a2(123), RecordEffect::Geometry)]);
}

#[test]
fn parameter_change_notifies_search_before_image_and_keeps_publication_pending() {
    for alternate in [false, true] {
        let mut measurement = Measurement::new();
        let mut backend = Backend::measuring();
        measurement.handle(control(MeasurementControl::Initialize), &mut backend);
        backend.enabling.insert(Lane::Search); backend.enabling.insert(Lane::Image); backend.alternate = alternate;
        backend.controls.push_back(RoundOutcome::NoWork);
        assert_eq!(measurement.handle(command(CommandKind::Command17), &mut backend), ChildOutcome::Pending);
        assert_eq!(backend.notifications, [if alternate { SearchNotification::WindowAlternate } else { SearchNotification::Window }]);
        assert!(!backend.publications.iter().any(|(_, effect)| *effect == RecordEffect::PublishMissing));
        backend.enabling = Lanes::empty();
        backend.events.push_back(RoundOutcome::Complete);
        assert_eq!(measurement.handle(completion(Completion::Image46), &mut backend), ChildOutcome::Complete);
        assert_eq!(backend.publications.last(), Some(&(stock::record_handle_from_a2(456), RecordEffect::PublishMissing)));
    }
}

#[test]
fn changed_results_keep_update_kind_when_restart_completes() {
    for update in [MeasurementUpdate::Window, MeasurementUpdate::NoPeerRequest, MeasurementUpdate::Source] {
        let mut measurement = Measurement::new();
        let mut backend = Backend::measuring();
        measurement.handle(control(MeasurementControl::Initialize), &mut backend);
        backend.controls.push_back(RoundOutcome::Pending);
        assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut backend), ChildOutcome::Pending);
        backend.enabling.insert(Lane::Source);
        backend.events.push_back(RoundOutcome::Updated(update));
        backend.controls.extend([RoundOutcome::Complete, RoundOutcome::Complete, RoundOutcome::Complete]);
        assert_eq!(measurement.handle(completion(Completion::Calculation8a), &mut backend), ChildOutcome::Measurement(MeasurementOutcome::Update { update, restart: Restart::Complete }));
    }
}

#[test]
fn changed_results_remain_pending_until_restart_join_completes() {
    let mut measurement = Measurement::new();
    let mut backend = Backend::measuring();
    measurement.handle(control(MeasurementControl::Initialize), &mut backend);
    backend.controls.push_back(RoundOutcome::Pending);
    measurement.handle(completion(Completion::Accessor4b), &mut backend);
    backend.events.push_back(RoundOutcome::Updated(MeasurementUpdate::Source));
    backend.controls.push_back(RoundOutcome::Pending);
    assert_eq!(measurement.handle(completion(Completion::Calculation8a), &mut backend), ChildOutcome::Measurement(MeasurementOutcome::Update { update: MeasurementUpdate::Source, restart: Restart::Pending }));
}
