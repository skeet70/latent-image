use crate::adjustment::{Adjustment, Completion, DriveAmount, Effect, Effects, ExposureDuration, ExposureLimits, ExposureMode, LensRequest, Percentage, Reply, Selection, SelectionLimit, exposure_duration};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Client(u8);

#[derive(Default)]
struct Traffic(Vec<Effect<Client>>);
impl Effects<Client> for Traffic {
    fn emit(&mut self, effect: Effect<Client>) {
        self.0.push(effect);
    }
}

#[test]
fn busy_request_replies_to_new_client_without_replacing_original() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.begin(Client(1), LensRequest::Synchronize, &mut traffic);
    adjustment.begin(Client(2), LensRequest::Stop, &mut traffic);
    adjustment.complete(Completion::Finished, &mut traffic);
    assert_eq!(traffic.0, [
        Effect::Request(LensRequest::Synchronize),
        Effect::Reply { callback: Client(2), result: Reply::Busy },
        Effect::Reply { callback: Client(1), result: Reply::Success },
    ]);
    traffic.0.clear();
    adjustment.begin(Client(3), LensRequest::Stop, &mut traffic);
    assert_eq!(traffic.0, [Effect::Request(LensRequest::Stop)]);
}

#[test]
fn progress_does_not_reply_or_release_request() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.begin(Client(1), LensRequest::Query, &mut traffic);
    traffic.0.clear();
    adjustment.complete(Completion::Progress, &mut traffic);
    assert!(traffic.0.is_empty());
    adjustment.begin(Client(2), LensRequest::Stop, &mut traffic);
    assert_eq!(traffic.0, [Effect::Reply { callback: Client(2), result: Reply::Busy }]);
}

#[test]
fn query_publishes_value_before_success_reply_and_failure_publishes_zero() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.begin(Client(1), LensRequest::Query, &mut traffic);
    traffic.0.clear();
    adjustment.complete(Completion::Query { success: true, value: 0x12345678 }, &mut traffic);
    assert_eq!(traffic.0, [
        Effect::PublishQuery { high: 0x123456, low: 0x78 },
        Effect::Reply { callback: Client(1), result: Reply::Success },
    ]);
    traffic.0.clear();
    adjustment.begin(Client(2), LensRequest::Query, &mut traffic);
    traffic.0.clear();
    adjustment.complete(Completion::Query { success: false, value: 0x12345678 }, &mut traffic);
    assert_eq!(traffic.0, [
        Effect::PublishQuery { high: 0, low: 0 },
        Effect::Reply { callback: Client(2), result: Reply::Success },
    ]);
}

#[test]
fn invalid_selection_reports_rejection_without_occupying_callback_slot() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.configure(Client(1), Selection(4), SelectionLimit(4), DriveAmount(9), &mut traffic);
    assert!(adjustment.selection_rejected());
    assert_eq!(traffic.0, [
        Effect::SelectionRejected,
        Effect::Reply { callback: Client(1), result: Reply::SelectionOutOfRange },
    ]);
    traffic.0.clear();
    adjustment.configure(Client(2), Selection(3), SelectionLimit(4), DriveAmount(9), &mut traffic);
    assert!(!adjustment.selection_rejected());
    assert_eq!(traffic.0, [Effect::Request(LensRequest::Configure { selection: Selection(3), value: DriveAmount(9) })]);
}

#[test]
fn busy_selection_does_not_change_saved_rejection() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.configure(Client(1), Selection(0), SelectionLimit(0), DriveAmount(0), &mut traffic);
    adjustment.begin(Client(2), LensRequest::Stop, &mut traffic);
    traffic.0.clear();
    adjustment.configure(Client(3), Selection(0), SelectionLimit(1), DriveAmount(0), &mut traffic);
    assert!(adjustment.selection_rejected());
    assert_eq!(traffic.0, [Effect::Reply { callback: Client(3), result: Reply::Busy }]);
}

#[test]
fn reserve_only_keeps_callback_without_peer_request() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.begin(Client(1), LensRequest::ReserveOnly, &mut traffic);
    assert!(traffic.0.is_empty());
    adjustment.begin(Client(2), LensRequest::Stop, &mut traffic);
    assert_eq!(traffic.0, [Effect::Reply { callback: Client(2), result: Reply::Busy }]);
}

#[test]
fn completed_callback_remains_the_recipient_of_a_late_completion() {
    let mut adjustment = Adjustment::new();
    let mut traffic = Traffic::default();
    adjustment.begin(Client(1), LensRequest::Stop, &mut traffic);
    adjustment.complete(Completion::Finished, &mut traffic);
    traffic.0.clear();
    adjustment.complete(Completion::Finished, &mut traffic);
    assert_eq!(traffic.0, [Effect::Reply { callback: Client(1), result: Reply::Success }]);
}

#[test]
fn exposure_duration_uses_mode_and_limits_without_reordering_limits() {
    let limits = ExposureLimits { minimum: ExposureDuration(100), maximum: ExposureDuration(1000) };
    assert_eq!(exposure_duration(ExposureMode::Off, ExposureDuration(500), limits), ExposureDuration(0));
    assert_eq!(exposure_duration(ExposureMode::Bounded, ExposureDuration(99), limits), ExposureDuration(100));
    assert_eq!(exposure_duration(ExposureMode::Bounded, ExposureDuration(500), limits), ExposureDuration(500));
    assert_eq!(exposure_duration(ExposureMode::Bounded, ExposureDuration(1001), limits), ExposureDuration(1000));
    assert_eq!(exposure_duration(ExposureMode::Maximum, ExposureDuration(0), limits), ExposureDuration(1000));
    assert_eq!(Percentage::saturating(101).get(), 100);
    assert_eq!(Percentage::saturating(99).get(), 99);
}
