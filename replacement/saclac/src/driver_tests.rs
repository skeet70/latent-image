use crate::driver::{self, Event, PayloadLimit, Ports};
use std::cell::Cell;
use std::rc::Rc;
use xm1_kernel::{fake::{Fake, FakeMessage}, Error, Header, Kernel, MessageQueue, QueueId, TaskId};

#[derive(Default)]
struct Effects {
    shared: Vec<u8>,
    delivered: Vec<Event>,
    faults: usize,
    releases: Rc<Cell<usize>>,
}
impl Ports for Effects {
    fn event(&self, operation: u16) -> Event { Event::new(u32::from(operation)) }
    fn command_capacity(&self, _: Event) -> Option<PayloadLimit> { Some(PayloadLimit::new(4)) }
    fn stage(&mut self, _: Header, payload: &[u8]) { self.shared = payload.to_vec(); }
    fn dispatch(&mut self, event: Event) {
        assert!(self.releases.get() > self.delivered.len());
        self.delivered.push(event);
    }
    fn fault(&mut self) { self.faults += 1; }
}

fn message(payload: &[u8], releases: &Rc<Cell<usize>>) -> FakeMessage {
    FakeMessage { header: Header { operation: 7, sender: 3, destination: 43 }, payload: payload.to_vec(), releases: Rc::clone(releases) }
}

#[test]
fn oversized_message_is_released_without_copy_or_delivery() {
    let mut effects = Effects { shared: vec![9; 4], ..Effects::default() };
    let kernel = Kernel::new(Fake::with_task(TaskId::new(43).unwrap())
        .receive_result(Ok(message(&[1; 5], &effects.releases))));
    let caller = kernel.task().unwrap();
    driver::receive_one(MessageQueue::stock(QueueId::new(43).unwrap()), &caller, &mut effects);
    assert_eq!(effects.shared, [9; 4]);
    assert!(effects.delivered.is_empty());
    assert_eq!(effects.faults, 1);
    assert_eq!(effects.releases.get(), 1);
}

#[test]
fn receive_failure_does_not_repeat_the_previous_delivery() {
    let mut effects = Effects::default();
    let kernel = Kernel::new(Fake::with_task(TaskId::new(43).unwrap())
        .receive_result(Ok(message(&[1, 2, 3, 4], &effects.releases)))
        .receive_result(Err(Error::Released))
        .receive_result(Ok(message(&[5, 6], &effects.releases))));
    let caller = kernel.task().unwrap();
    let queue = MessageQueue::stock(QueueId::new(43).unwrap());
    driver::receive_one(queue, &caller, &mut effects);
    driver::receive_one(queue, &caller, &mut effects);
    assert_eq!(effects.shared, [1, 2, 3, 4]);
    assert_eq!(effects.delivered, [Event::new(7)]);
    assert_eq!(effects.faults, 1);
    driver::receive_one(queue, &caller, &mut effects);
    assert_eq!(effects.shared, [5, 6]);
    assert_eq!(effects.delivered, [Event::new(7), Event::new(7)]);
    assert_eq!(effects.releases.get(), 2);
}
