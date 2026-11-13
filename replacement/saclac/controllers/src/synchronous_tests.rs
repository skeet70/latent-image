use crate::synchronous::{Admission, Clients, Completion, Effect, Effects};
use std::vec::Vec;
use xm1_saclac_interface::{stock, ClientCallback, CompletionStatus};

#[derive(Default)]
struct Traffic(Vec<Effect>);
impl Effects for Traffic {
    fn emit(&mut self, effect: Effect) { self.0.push(effect); }
}

fn callback(value: u32) -> ClientCallback {
    stock::optional_callback(value).unwrap()
}

#[test]
fn busy_request_replies_to_new_client_without_replacing_the_waiter() {
    let first = callback(1);
    let second = callback(2);
    let mut clients = Clients::new();
    let mut traffic = Traffic::default();
    assert_eq!(clients.reserve(first, &mut traffic), Admission::Accepted);
    assert_eq!(clients.reserve(second, &mut traffic), Admission::Busy);
    clients.complete(Completion::Finished, &mut traffic);
    assert_eq!(traffic.0, [
        Effect::Occupied,
        Effect::Reply { callback: second, status: CompletionStatus(-95) },
        Effect::Reply { callback: first, status: CompletionStatus(0) },
        Effect::Released,
    ]);
}

#[test]
fn progress_keeps_the_slot_and_completion_releases_after_client_reply() {
    let first = callback(1);
    let second = callback(2);
    let mut clients = Clients::new();
    let mut traffic = Traffic::default();
    clients.reserve(first, &mut traffic);
    traffic.0.clear();
    clients.complete(Completion::Progress, &mut traffic);
    assert!(traffic.0.is_empty());
    assert_eq!(clients.reserve(second, &mut traffic), Admission::Busy);
    clients.complete(Completion::Finished, &mut traffic);
    assert_eq!(clients.reserve(second, &mut traffic), Admission::Accepted);
    assert_eq!(traffic.0, [
        Effect::Reply { callback: second, status: CompletionStatus(-95) },
        Effect::Reply { callback: first, status: CompletionStatus(0) },
        Effect::Released,
        Effect::Occupied,
    ]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueryTraffic {
    PositionAvailability,
    Position,
    MotionAvailability,
    Motion,
    MissingLens,
    ImagePrimary,
    ImageSecondary,
    Movie(crate::synchronous::MoviePhase),
}

struct QueryServices {
    availability: crate::synchronous::Availability,
    traffic: Vec<QueryTraffic>,
}

impl crate::synchronous::Queries for QueryServices {
    fn position_availability(&mut self) -> crate::synchronous::Availability {
        self.traffic.push(QueryTraffic::PositionAvailability);
        self.availability
    }
    fn position(&mut self) -> crate::synchronous::LensPositionCode {
        self.traffic.push(QueryTraffic::Position);
        crate::synchronous::LensPositionCode(0x12345678)
    }
    fn motion_availability(&mut self) -> crate::synchronous::Availability {
        self.traffic.push(QueryTraffic::MotionAvailability);
        self.availability
    }
    fn motion(&mut self) -> crate::adjustment::LensPosition {
        self.traffic.push(QueryTraffic::Motion);
        crate::adjustment::LensPosition(-123)
    }
    fn report_missing_lens(&mut self) { self.traffic.push(QueryTraffic::MissingLens); }
    fn image_primary(&mut self) -> crate::synchronous::ImageStatus {
        self.traffic.push(QueryTraffic::ImagePrimary);
        crate::synchronous::ImageStatus(7)
    }
    fn image_secondary(&mut self) -> crate::synchronous::ImageStatus {
        self.traffic.push(QueryTraffic::ImageSecondary);
        crate::synchronous::ImageStatus(8)
    }
    fn movie_readiness(&mut self, phase: crate::synchronous::MoviePhase) -> crate::synchronous::Readiness {
        self.traffic.push(QueryTraffic::Movie(phase));
        match phase {
            crate::synchronous::MoviePhase::Primary => crate::synchronous::Readiness::Ready,
            crate::synchronous::MoviePhase::Secondary => crate::synchronous::Readiness::Busy,
        }
    }
}

#[test]
fn available_lens_queries_return_peer_values_in_request_order() {
    use crate::synchronous::{lens_motion, lens_position, Availability, LensPositionCode};
    let mut services = QueryServices { availability: Availability::Available, traffic: Vec::new() };
    assert_eq!(lens_position(&mut services), LensPositionCode(0x12345678));
    assert_eq!(lens_motion(&mut services), crate::adjustment::LensPosition(-123));
    assert_eq!(services.traffic, [QueryTraffic::PositionAvailability, QueryTraffic::Position,
        QueryTraffic::MotionAvailability, QueryTraffic::Motion]);
}

#[test]
fn missing_lens_queries_return_fallbacks_without_querying_the_lens() {
    use crate::synchronous::{lens_motion, lens_position, Availability, LensPositionCode};
    let mut services = QueryServices { availability: Availability::Unavailable, traffic: Vec::new() };
    assert_eq!(lens_position(&mut services), LensPositionCode(128));
    assert_eq!(lens_motion(&mut services), crate::adjustment::LensPosition(0));
    assert_eq!(services.traffic, [QueryTraffic::PositionAvailability,
        QueryTraffic::MotionAvailability, QueryTraffic::MissingLens]);
}

#[test]
fn parameter_notifications_do_not_occupy_or_release_the_lens_client() {
    let mut traffic = Traffic::default();
    crate::synchronous::notify(None, &mut traffic);
    crate::synchronous::notify(Some(callback(1)), &mut traffic);
    assert_eq!(traffic.0, [Effect::Reply { callback: callback(1), status: CompletionStatus(0) }]);
}

#[test]
fn movie_status_preserves_phase_and_maps_busy_at_the_boundary() {
    use crate::synchronous::{movie_status, Availability, MoviePhase};
    let mut services = QueryServices { availability: Availability::Available, traffic: Vec::new() };
    assert_eq!(crate::stock::readiness_status(movie_status(MoviePhase::Primary, &mut services)), CompletionStatus(0));
    assert_eq!(crate::stock::readiness_status(movie_status(MoviePhase::Secondary, &mut services)), CompletionStatus(-94));
    assert_eq!(services.traffic, [QueryTraffic::Movie(MoviePhase::Primary), QueryTraffic::Movie(MoviePhase::Secondary)]);
}
