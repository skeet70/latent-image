use crate::lifecycle::{Effects, Event, Lifecycle, Outcome, Request, Service, Startup};
use std::vec::Vec;

#[derive(Default)]
struct Traffic(Vec<Request>);
impl Effects for Traffic {
    fn emit(&mut self, request: Request) {
        self.0.push(request);
    }
}

#[test]
fn normal_start_publishes_parameters_before_reply() {
    let mut controller = Lifecycle::new();
    let mut traffic = Traffic::default();
    assert_eq!(controller.handle(Event::Start(Startup::Normal), &mut traffic), Outcome::Started);
    assert_eq!(traffic.0, [
        Request::ReserveAccessor,
        Request::Start(Service::Lens),
        Request::Start(Service::RecordingBuffers),
        Request::ResetImageBuffer,
        Request::Start(Service::Image),
        Request::Start(Service::Calculation),
        Request::Start(Service::AuxiliaryGate),
        Request::Start(Service::Statistics),
        Request::PublishParameters,
        Request::ReplyStarted,
    ]);
}

#[test]
fn resume_start_does_not_restart_lens_service() {
    let mut controller = Lifecycle::new();
    let mut traffic = Traffic::default();
    assert_eq!(controller.handle(Event::Start(Startup::ResumeLive), &mut traffic), Outcome::Live);
    assert_eq!(traffic.0, [
        Request::ReserveAccessor,
        Request::Start(Service::RecordingBuffers),
        Request::ResetImageBuffer,
        Request::Start(Service::Image),
        Request::Start(Service::Calculation),
        Request::Start(Service::AuxiliaryGate),
        Request::Start(Service::Statistics),
        Request::PublishParameters,
        Request::ReplyStarted,
    ]);
}

#[test]
fn stop_finishes_peers_before_reply_and_allows_restart() {
    let mut controller = Lifecycle::new();
    let mut traffic = Traffic::default();
    controller.handle(Event::Start(Startup::Normal), &mut traffic);
    traffic.0.clear();
    assert_eq!(controller.handle(Event::Stop, &mut traffic), Outcome::Stopped);
    assert_eq!(traffic.0, [
        Request::Stop(Service::Image),
        Request::Stop(Service::Calculation),
        Request::Stop(Service::AuxiliaryGate),
        Request::Stop(Service::Statistics),
        Request::Stop(Service::Lens),
        Request::Stop(Service::RecordingBuffers),
        Request::ReplyStopped,
    ]);
    assert_eq!(controller.handle(Event::Start(Startup::Normal), &mut traffic), Outcome::Started);
}

#[test]
fn live_teardown_requires_lens_stop_before_service_stop() {
    let mut controller = Lifecycle::new();
    let mut traffic = Traffic::default();
    controller.handle(Event::Start(Startup::Normal), &mut traffic);
    assert_eq!(controller.handle(Event::LensReady, &mut traffic), Outcome::Live);
    traffic.0.clear();
    assert_eq!(controller.handle(Event::Stop, &mut traffic), Outcome::InvalidEvent);
    assert!(traffic.0.is_empty());
    assert_eq!(controller.handle(Event::LensStopped, &mut traffic), Outcome::Started);
    assert_eq!(controller.handle(Event::Stop, &mut traffic), Outcome::Stopped);
}

#[test]
fn repeated_start_republishes_parameters_but_live_start_is_refused() {
    let mut controller = Lifecycle::new();
    let mut traffic = Traffic::default();
    assert_eq!(controller.handle(Event::Stop, &mut traffic), Outcome::InvalidEvent);
    assert!(traffic.0.is_empty());
    controller.handle(Event::Start(Startup::Normal), &mut traffic);
    let first = core::mem::take(&mut traffic.0);
    assert_eq!(controller.handle(Event::Start(Startup::Normal), &mut traffic), Outcome::Started);
    assert_eq!(traffic.0, first);
    controller.handle(Event::LensReady, &mut traffic);
    traffic.0.clear();
    assert_eq!(controller.handle(Event::Start(Startup::Normal), &mut traffic), Outcome::InvalidEvent);
    assert!(traffic.0.is_empty());
}
