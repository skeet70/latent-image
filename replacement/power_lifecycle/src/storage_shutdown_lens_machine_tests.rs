use super::*;
use crate::shutdown::RequestId;
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    Reset,
    Initialize,
    StoppedCommand,
    StoppedWrite,
    StoppedRead,
    QueueControl,
    CommandResponse,
    WritePage,
    WriteResponse,
    ReadPage,
    ReadResponse,
}

struct Recorder {
    operation: RequestId,
    calls: Vec<Call>,
    command_response: Option<LensServiceEvent>,
    write_response: Option<LensServiceEvent>,
    read_response: Option<LensServiceEvent>,
    invalid: Option<(u32, LensServiceEvent)>,
    unexpected: Option<LensServiceState>,
}
impl Recorder {
    fn new(operation: RequestId) -> Self {
        Self {
            operation,
            calls: Vec::new(),
            command_response: None,
            write_response: None,
            read_response: None,
            invalid: None,
            unexpected: None,
        }
    }
}
impl LensServiceHandlers for Recorder {
    fn operation(&self) -> RequestId { self.operation }
    fn reset(&mut self) { self.calls.push(Call::Reset) }
    fn initialize(&mut self) { self.calls.push(Call::Initialize) }
    fn stopped_command(&mut self) { self.calls.push(Call::StoppedCommand) }
    fn stopped_write(&mut self) { self.calls.push(Call::StoppedWrite) }
    fn stopped_read(&mut self) { self.calls.push(Call::StoppedRead) }
    fn queue_control(&mut self) { self.calls.push(Call::QueueControl) }
    fn command_response(&mut self) -> Option<LensServiceEvent> {
        self.calls.push(Call::CommandResponse);
        self.command_response
    }
    fn write_page(&mut self) { self.calls.push(Call::WritePage) }
    fn write_response(&mut self) -> Option<LensServiceEvent> {
        self.calls.push(Call::WriteResponse);
        self.write_response
    }
    fn read_page(&mut self) { self.calls.push(Call::ReadPage) }
    fn read_response(&mut self) -> Option<LensServiceEvent> {
        self.calls.push(Call::ReadResponse);
        self.read_response
    }
    fn invalid_state(&mut self, state: u32, event: LensServiceEvent) { self.invalid = Some((state, event)) }
    fn unexpected_initialize(&mut self, state: LensServiceState) { self.unexpected = Some(state) }
}

struct Machine {
    history: [u32; 4],
    trace: [LensServiceTrace; 16],
    cursor: u32,
}
impl Machine {
    fn new(state: u32) -> Self {
        Self { history: [state, 0, 0, 0], trace: [LensServiceTrace { state: 0, event: 0 }; 16], cursor: 0 }
    }
    fn dispatch(&mut self, handlers: &mut impl LensServiceHandlers, event: LensServiceEvent) {
        LensServiceMachine::new(&mut self.history, &mut self.trace, &mut self.cursor).dispatch(handlers, event);
    }
}

fn trace(state: u32, event: LensServiceEvent) -> LensServiceTrace {
    LensServiceTrace { state, event: event.abi() }
}

#[test]
fn stopped_initialize_runs_the_handler_and_enters_running() {
    let mut machine = Machine::new(LensServiceState::Stopped as u32);
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::Initialize);
    assert_eq!(recorder.calls, std::vec![Call::Initialize]);
    assert_eq!(machine.history, [LensServiceState::Running as u32, LensServiceState::Stopped as u32, 0, 0]);
    assert_eq!(machine.trace[0], trace(LensServiceState::Stopped as u32, LensServiceEvent::Initialize));
    assert_eq!(machine.trace[1], trace(LensServiceState::Running as u32, LensServiceEvent::Enter));
    assert_eq!(machine.trace[2], LensServiceTrace { state: 0, event: 0 });
    assert_eq!(machine.cursor, 2);
}

#[test]
fn running_stop_returns_to_stopped_and_runs_enter() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::Stop);
    assert_eq!(recorder.calls, std::vec![Call::Reset]);
    assert_eq!(machine.history, [LensServiceState::Stopped as u32, LensServiceState::Running as u32, 0, 0]);
    assert_eq!(machine.trace[0], trace(LensServiceState::Running as u32, LensServiceEvent::Stop));
    assert_eq!(machine.trace[1], trace(LensServiceState::Stopped as u32, LensServiceEvent::Enter));
}

#[test]
fn received_dispatches_the_event_classified_from_the_handler_operation() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0x7251));
    machine.dispatch(&mut recorder, LensServiceEvent::Received);
    assert_eq!(recorder.calls, std::vec![Call::CommandResponse]);
    assert_eq!(machine.trace[0], trace(LensServiceState::Running as u32, LensServiceEvent::Received));
    assert_eq!(machine.trace[1], trace(LensServiceState::Running as u32, LensServiceEvent::CommandResponse));
    assert_eq!(machine.history[0], LensServiceState::Running as u32);
}

#[test]
fn received_with_an_unclassified_operation_stops_after_recording_it() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0x7100));
    machine.dispatch(&mut recorder, LensServiceEvent::Received);
    assert!(recorder.calls.is_empty());
    assert_eq!(machine.trace[0], trace(LensServiceState::Running as u32, LensServiceEvent::Received));
    assert_eq!(machine.cursor, 1);
}

#[test]
fn stopped_command_write_and_read_reach_the_stopped_handlers() {
    for (event, call) in [
        (LensServiceEvent::Command, Call::StoppedCommand),
        (LensServiceEvent::Write, Call::StoppedWrite),
        (LensServiceEvent::Read, Call::StoppedRead),
    ] {
        let mut machine = Machine::new(LensServiceState::Stopped as u32);
        let mut recorder = Recorder::new(RequestId(0));
        machine.dispatch(&mut recorder, event);
        assert_eq!(recorder.calls, std::vec![call]);
        assert_eq!(machine.history[0], LensServiceState::Stopped as u32);
    }
}

#[test]
fn running_command_write_and_read_reach_the_running_handlers() {
    for (event, call) in [
        (LensServiceEvent::Command, Call::QueueControl),
        (LensServiceEvent::Write, Call::WritePage),
        (LensServiceEvent::Read, Call::ReadPage),
    ] {
        let mut machine = Machine::new(LensServiceState::Running as u32);
        let mut recorder = Recorder::new(RequestId(0));
        machine.dispatch(&mut recorder, event);
        assert_eq!(recorder.calls, std::vec![call]);
        assert_eq!(machine.history[0], LensServiceState::Running as u32);
    }
}

#[test]
fn running_initialize_is_reported_without_leaving_running() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::Initialize);
    assert!(recorder.calls.is_empty());
    assert_eq!(recorder.unexpected, Some(LensServiceState::Running));
    assert_eq!(machine.history[0], LensServiceState::Running as u32);
}

#[test]
fn unknown_state_word_is_reported_with_the_event_and_stops_the_machine() {
    let mut machine = Machine::new(7);
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::Command);
    assert!(recorder.calls.is_empty());
    assert_eq!(recorder.invalid, Some((7, LensServiceEvent::Command)));
    assert_eq!(machine.history[0], 7);
}

#[test]
fn response_handler_event_is_dispatched_in_the_same_running_state() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0x7251));
    recorder.command_response = Some(LensServiceEvent::Write);
    machine.dispatch(&mut recorder, LensServiceEvent::Received);
    assert_eq!(recorder.calls, std::vec![Call::CommandResponse, Call::WritePage]);
    assert_eq!(machine.trace[1], trace(LensServiceState::Running as u32, LensServiceEvent::CommandResponse));
    assert_eq!(machine.trace[2], trace(LensServiceState::Running as u32, LensServiceEvent::Write));
}

#[test]
fn response_event_without_a_chained_event_finishes_the_dispatch() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::WriteResponse);
    assert_eq!(recorder.calls, std::vec![Call::WriteResponse]);
    assert_eq!(machine.cursor, 1);
}

#[test]
fn trace_cursor_wraps_and_clears_the_next_slot() {
    let mut machine = Machine::new(LensServiceState::Running as u32);
    machine.cursor = 15;
    let mut recorder = Recorder::new(RequestId(0));
    machine.dispatch(&mut recorder, LensServiceEvent::WriteResponse);
    assert_eq!(machine.cursor, 0);
    assert_eq!(machine.trace[15], trace(LensServiceState::Running as u32, LensServiceEvent::WriteResponse));
    assert_eq!(machine.trace[0], LensServiceTrace { state: 0, event: 0 });
}

#[test]
fn history_retains_the_last_four_states() {
    let mut machine = Machine::new(LensServiceState::Stopped as u32);
    let mut recorder = Recorder::new(RequestId(0));
    for _ in 0..3 {
        machine.dispatch(&mut recorder, LensServiceEvent::Initialize);
        machine.dispatch(&mut recorder, LensServiceEvent::Stop);
    }
    assert_eq!(machine.history, [1, 2, 1, 2]);
}

#[test]
fn event_classification_covers_the_operation_range() {
    assert_eq!(LensServiceEvent::classify(RequestId(0x7200)), Some(LensServiceEvent::Initialize));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7201)), Some(LensServiceEvent::Stop));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7202)), Some(LensServiceEvent::Command));
    assert_eq!(LensServiceEvent::classify(RequestId(0x722e)), Some(LensServiceEvent::Command));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7251)), Some(LensServiceEvent::CommandResponse));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7252)), Some(LensServiceEvent::Write));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7253)), Some(LensServiceEvent::WriteResponse));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7254)), Some(LensServiceEvent::Read));
    assert_eq!(LensServiceEvent::classify(RequestId(0x7255)), Some(LensServiceEvent::ReadResponse));
    assert_eq!(LensServiceEvent::classify(RequestId(0x725c)), Some(LensServiceEvent::Command));
    assert_eq!(LensServiceEvent::classify(RequestId(0x725d)), None);
    assert_eq!(LensServiceEvent::classify(RequestId(0x71ff)), None);
}

#[test]
fn state_words_are_one_and_two_only() {
    assert_eq!(LensServiceState::from_abi(1), Some(LensServiceState::Stopped));
    assert_eq!(LensServiceState::from_abi(2), Some(LensServiceState::Running));
    assert_eq!(LensServiceState::from_abi(0), None);
    assert_eq!(LensServiceState::from_abi(3), None);
}
