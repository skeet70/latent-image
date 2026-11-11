use crate::shutdown::RequestId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LensServiceEvent { Enter, Received, Initialize, Stop, Command, CommandResponse, Write, WriteResponse, Read, ReadResponse }
impl LensServiceEvent {
    pub const fn abi(self) -> u32 { self as u32 }
    pub const fn classify(operation: RequestId) -> Option<Self> {
        match operation.0 {
            0x7200 => Some(Self::Initialize), 0x7201 => Some(Self::Stop),
            0x7251 => Some(Self::CommandResponse), 0x7252 => Some(Self::Write),
            0x7253 => Some(Self::WriteResponse), 0x7254 => Some(Self::Read),
            0x7255 => Some(Self::ReadResponse), 0x7202..=0x725c => Some(Self::Command),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LensServiceState { Stopped = 1, Running = 2 }
impl LensServiceState {
    pub const fn from_abi(value: u32) -> Option<Self> { match value { 1 => Some(Self::Stopped), 2 => Some(Self::Running), _ => None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct LensServiceTrace { pub state: u32, pub event: u32 }

pub trait LensServiceHandlers {
    fn operation(&self) -> RequestId;
    fn reset(&mut self);
    fn initialize(&mut self);
    fn stopped_command(&mut self);
    fn stopped_write(&mut self);
    fn stopped_read(&mut self);
    fn queue_control(&mut self);
    fn command_response(&mut self) -> Option<LensServiceEvent>;
    fn write_page(&mut self);
    fn write_response(&mut self) -> Option<LensServiceEvent>;
    fn read_page(&mut self);
    fn read_response(&mut self) -> Option<LensServiceEvent>;
    fn invalid_state(&mut self, state: u32, event: LensServiceEvent);
    fn unexpected_initialize(&mut self, state: LensServiceState);
}

pub struct LensServiceMachine<'a> {
    history: &'a mut [u32; 4],
    trace: &'a mut [LensServiceTrace; 16],
    cursor: &'a mut u32,
}
impl<'a> LensServiceMachine<'a> {
    pub fn new(history: &'a mut [u32; 4], trace: &'a mut [LensServiceTrace; 16], cursor: &'a mut u32) -> Self { Self { history, trace, cursor } }
    pub fn dispatch(&mut self, handlers: &mut impl LensServiceHandlers, mut event: LensServiceEvent) {
        loop {
            let state_word = self.history[0];
            self.trace[*self.cursor as usize] = LensServiceTrace { state: state_word, event: event.abi() };
            *self.cursor = if *self.cursor <= 14 { *self.cursor + 1 } else { 0 };
            self.trace[*self.cursor as usize] = LensServiceTrace { state: 0, event: 0 };
            let Some(state) = LensServiceState::from_abi(state_word) else { handlers.invalid_state(state_word, event); return; };
            let mut next_state = None;
            let next_event = match (state, event) {
                (_, LensServiceEvent::Received) => LensServiceEvent::classify(handlers.operation()),
                (LensServiceState::Stopped, LensServiceEvent::Enter) => { handlers.reset(); None }
                (LensServiceState::Stopped, LensServiceEvent::Initialize) => { handlers.initialize(); next_state = Some(LensServiceState::Running); None }
                (_, LensServiceEvent::Stop) => { next_state = Some(LensServiceState::Stopped); None }
                (LensServiceState::Stopped, LensServiceEvent::Command) => { handlers.stopped_command(); None }
                (LensServiceState::Stopped, LensServiceEvent::Write) => { handlers.stopped_write(); None }
                (LensServiceState::Stopped, LensServiceEvent::Read) => { handlers.stopped_read(); None }
                (LensServiceState::Running, LensServiceEvent::Initialize) => { handlers.unexpected_initialize(state); None }
                (LensServiceState::Running, LensServiceEvent::Command) => { handlers.queue_control(); None }
                (LensServiceState::Running, LensServiceEvent::CommandResponse) => handlers.command_response(),
                (LensServiceState::Running, LensServiceEvent::Write) => { handlers.write_page(); None }
                (LensServiceState::Running, LensServiceEvent::WriteResponse) => handlers.write_response(),
                (LensServiceState::Running, LensServiceEvent::Read) => { handlers.read_page(); None }
                (LensServiceState::Running, LensServiceEvent::ReadResponse) => handlers.read_response(),
                _ => None,
            };
            if let Some(next_event) = next_event { event = next_event; continue; }
            let Some(next_state) = next_state else { return; };
            self.history[3] = self.history[2];
            self.history[2] = self.history[1];
            self.history[1] = self.history[0];
            self.history[0] = next_state as u32;
            event = LensServiceEvent::Enter;
        }
    }
}

#[cfg(test)]
#[path = "storage_shutdown_lens_machine_tests.rs"]
mod tests;
