use crate::machine0::{self, Call, Services, Source, State, WorkState};
use crate::DevicePacket;

#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Status(Source),
    Call(Call),
    State(State),
    Work(WorkState),
    Device(u32, u32),
    ClearBufferStep,
}

struct Script {
    statuses: Vec<i32>,
    mode_value: u32,
    effects: Vec<Effect>,
}

impl Script {
    fn new(statuses: &[i32]) -> Self {
        Self { statuses: statuses.to_vec(), mode_value: 0x1234, effects: Vec::new() }
    }
}

impl Services for Script {
    fn status(&mut self, source: Source) -> i32 {
        self.effects.push(Effect::Status(source));
        self.statuses.remove(0)
    }
    fn call(&mut self, call: Call) {
        self.effects.push(Effect::Call(call));
    }
    fn set_state(&mut self, state: State) {
        self.effects.push(Effect::State(state));
    }
    fn set_work_state(&mut self, state: WorkState) {
        self.effects.push(Effect::Work(state));
    }
    fn notify_device(&mut self, packet: &DevicePacket) {
        self.effects.push(Effect::Device(packet.code, packet.value));
    }
    fn mode_value(&self) -> u32 {
        self.mode_value
    }
    fn clear_buffer_step(&mut self) {
        self.effects.push(Effect::ClearBufferStep);
    }
}

fn run(action: fn(&mut Script) -> i32, statuses: &[i32]) -> (i32, Vec<Effect>) {
    let mut script = Script::new(statuses);
    let result = action(&mut script);
    assert!(script.statuses.is_empty(), "unread statuses {:?}", script.statuses);
    (result, script.effects)
}

#[test]
fn start_status_enters_primary_work_and_publishes() {
    let (result, effects) = run(machine0::action13, &[8]);
    assert_eq!(result, 0);
    assert_eq!(
        effects,
        [
            Effect::Status(Source::Navigation),
            Effect::State(State::Primary),
            Effect::Call(Call::NotifyPrimary),
            Effect::Work(WorkState::Primary),
            Effect::Call(Call::PublishRequestedMode),
        ]
    );
}

#[test]
fn abort_status_posts_and_returns_255() {
    let (result, effects) = run(machine0::action32, &[255]);
    assert_eq!(result, 255);
    assert_eq!(effects[1..], [Effect::State(State::Posted), Effect::Call(Call::PostPendingEvent)]);
}

#[test]
fn unknown_status_resets_to_callback_ready() {
    let (result, effects) = run(machine0::action02, &[7]);
    assert_eq!(result, 0);
    assert_eq!(
        effects,
        [Effect::Call(Call::ResetWorkspaces), Effect::Status(Source::Parameter), Effect::Call(Call::CallbackReady)]
    );
}

#[test]
fn mode_packet_carries_the_mode_value() {
    let (result, effects) = run(machine0::action16, &[13]);
    assert_eq!(result, 0);
    assert_eq!(effects[1..], [Effect::State(State::Work), Effect::Device(0x81e, 0x1234)]);
}

#[test]
fn packet_082d_keeps_state_only_in_action21() {
    let (result21, effects21) = run(machine0::action21, &[17]);
    let (result22, effects22) = run(machine0::action22, &[17]);
    assert_eq!((result21, result22), (17, 17));
    assert!(!effects21.contains(&Effect::State(State::Work)));
    assert!(effects22.contains(&Effect::State(State::Work)));
}

#[test]
fn second_status_is_read_only_after_navigation_start() {
    assert_eq!(run(machine0::action05, &[0, 4]).0, 0);
    let (result, effects) = run(machine0::action05, &[0, 8, 4]);
    assert_eq!(result, 3);
    assert_eq!(effects.last(), Some(&Effect::Call(Call::PostPendingEvent)));
    let (result, effects) = run(machine0::action05, &[0, 8, 5]);
    assert_eq!(result, 0);
    assert_eq!(effects.last(), Some(&Effect::State(State::Primary)));
}

#[test]
fn error_in_buffer_actions_restarts_primary_work() {
    let (result, effects) = run(machine0::action07, &[12]);
    assert_eq!(result, 0);
    assert_eq!(
        effects[1..],
        [
            Effect::State(State::Primary),
            Effect::Call(Call::SourceErrorNotification),
            Effect::Work(WorkState::Primary),
            Effect::Call(Call::PublishRequestedMode),
        ]
    );
}
