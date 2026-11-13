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
