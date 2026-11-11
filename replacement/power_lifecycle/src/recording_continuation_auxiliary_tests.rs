use super::{dispatch, Field, Request, Runtime};

#[derive(Default)]
struct Auxiliary {
    words: [u32; 8],
    state: u16,
    operation: u16,
    value: u32,
    completion: u32,
    published_started: bool,
    waits: Vec<u32>,
    deferred: usize,
    reports: usize,
    wait_status: i32,
}
fn index(field: Field) -> usize { (field as usize - 0xb488) / 4 }
impl Runtime for Auxiliary {
    fn word(&self, field: Field) -> u32 { self.words[index(field)] }
    fn set_word(&mut self, field: Field, value: u32) { self.words[index(field)] = value; }
    fn state(&self) -> u16 { self.state }
    fn set_state(&mut self, state: u16) { self.state = state; }
    fn set_event(&mut self, _: u16) {}
    fn current_operation(&self) -> u16 { self.operation }
    fn message_value(&self) -> u32 { self.value }
    fn completion_value(&self) -> u32 { self.completion }
    fn defer_current(&mut self) { self.deferred += 1; }
    fn report_missing_state(&mut self) { self.reports += 1; }
    fn publish_started(&mut self) { self.published_started = true; }
    fn wait(&mut self, mode: u32) -> i32 { self.waits.push(mode); self.wait_status }
}

#[test]
fn start_waits_for_its_started_message_before_reporting_ready() {
    let mut auxiliary = Auxiliary::default();
    auxiliary.set_word(Field::Value, 1);
    assert_eq!(dispatch(&mut auxiliary, Request::Start), Ok(0));
    assert!(auxiliary.published_started);
    assert_eq!(auxiliary.state(), 1);
    assert_eq!(auxiliary.word(Field::Response), 1);
    assert_eq!(auxiliary.word(Field::Completion), 0);
    auxiliary.operation = 0x13c9;
    auxiliary.value = 1;
    assert_eq!(dispatch(&mut auxiliary, Request::CompletionEvent), Ok(1));
    assert_eq!(auxiliary.state(), 3);
    assert_eq!(auxiliary.word(Field::Response), 3);
    assert_eq!(auxiliary.word(Field::Completion), 1);
}

#[test]
fn waiting_retains_pending_state_until_actual_timer_message() {
    let mut auxiliary = Auxiliary { state: 3, ..Auxiliary::default() };
    auxiliary.set_word(Field::Value, 2);
    assert_eq!(dispatch(&mut auxiliary, Request::Wait), Ok(0));
    assert_eq!(auxiliary.waits, [3]);
    assert_eq!(auxiliary.state(), 2);
    assert_eq!(auxiliary.word(Field::Response), 2);
    auxiliary.operation = 0x13c8;
    auxiliary.value = 3;
    assert_eq!(dispatch(&mut auxiliary, Request::TimerEvent), Ok(1));
    assert_eq!(auxiliary.state(), 3);
    assert_eq!(auxiliary.word(Field::Completion), 3);
}

#[test]
fn stop_clears_state_only_after_timer_callback() {
    let mut auxiliary = Auxiliary { state: 3, ..Auxiliary::default() };
    auxiliary.set_word(Field::Completion, 99);
    assert_eq!(dispatch(&mut auxiliary, Request::Stop), Ok(0));
    assert_eq!(auxiliary.waits, [0]);
    assert_eq!(auxiliary.state(), 5);
    assert_eq!(auxiliary.word(Field::Completion), 99);
    auxiliary.operation = 0x13c8;
    assert_eq!(dispatch(&mut auxiliary, Request::TimerEvent), Ok(1));
    assert_eq!(auxiliary.state(), 0);
    assert_eq!(auxiliary.word(Field::Response), 0);
    assert_eq!(auxiliary.word(Field::Completion), 0);
}

#[test]
fn query_reports_all_six_valid_states_without_changing_them() {
    for state in 0..6 {
        let mut auxiliary = Auxiliary { state, ..Auxiliary::default() };
        assert_eq!(dispatch(&mut auxiliary, Request::Query), Ok(i32::from(state == 0 || state == 3)));
        assert_eq!(auxiliary.word(Field::Response), state as u32);
        assert_eq!(auxiliary.state(), state);
    }
}

#[test]
fn busy_start_defers_without_publishing_another_start() {
    for state in 1..6 {
        let mut auxiliary = Auxiliary { state, ..Auxiliary::default() };
        assert_eq!(dispatch(&mut auxiliary, Request::Start), Ok(0));
        assert_eq!(auxiliary.deferred, 1);
        assert!(!auxiliary.published_started);
        assert_eq!(auxiliary.state(), state);
    }
}

#[test]
fn wrong_completion_operation_reports_without_accepting_payload() {
    let mut auxiliary = Auxiliary { state: 1, operation: 0x13c8, value: 7, ..Auxiliary::default() };
    assert_eq!(dispatch(&mut auxiliary, Request::CompletionEvent), Ok(1));
    assert_eq!(auxiliary.reports, 1);
    assert_eq!(auxiliary.word(Field::Completion), 0);
}
