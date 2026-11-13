#![forbid(unsafe_code)]

use xm1_kernel::{Backend, Header, MessageQueue, TaskContext, Timeout};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event(u32);
impl Event {
    pub const fn new(code: u32) -> Self { Self(code) }
    pub const fn code(self) -> u32 { self.0 }
}

pub trait Ports {
    fn event(&self, operation: u16) -> Event;
    fn command_capacity(&self, event: Event) -> Option<usize>;
    fn stage(&mut self, header: Header, payload: &[u8]);
    fn dispatch(&mut self, event: Event);
    fn fault(&mut self);
}

pub fn receive_one<B: Backend>(queue: MessageQueue, caller: &TaskContext<'_, B>, ports: &mut impl Ports) {
    let received = match queue.receive(caller, Timeout::Forever) {
        Ok(received) => received,
        Err(_) => {
            ports.fault();
            return;
        }
    };
    let header = received.header();
    let event = ports.event(header.operation);
    if ports.command_capacity(event).is_some_and(|capacity| received.payload().len() > capacity) {
        drop(received);
        ports.fault();
        return;
    }
    ports.stage(header, received.payload());
    drop(received);
    ports.dispatch(event);
}

pub fn run<B: Backend>(queue: MessageQueue, caller: &TaskContext<'_, B>, ports: &mut impl Ports) -> ! {
    loop { receive_one(queue, caller, ports); }
}
