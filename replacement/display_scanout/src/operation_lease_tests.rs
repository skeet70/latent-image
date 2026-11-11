use crate::operation_lease::*;
use crate::Engine;

struct Runtime { initialized: bool, count: u8, reset: bool, selected: Engine }
impl OperationLease for Runtime {
    fn selected(&self) -> Engine { self.selected }
    fn initialized(&self, _: Engine) -> bool { self.initialized }
    fn count(&self, _: Engine) -> u8 { self.count }
    fn set_count(&mut self, _: Engine, value: u8) { self.count = value; }
    fn reset(&mut self, _: Engine) { self.reset = true; }
}
fn runtime() -> Runtime { Runtime { initialized: true, count: 0, reset: false, selected: Engine::One } }

#[test]
fn uninitialized_engine_refuses_both_operations() {
    let mut state = Runtime { initialized: false, count: 2, reset: false, selected: Engine::Zero };
    assert_eq!(acquire(&mut state, Engine::Zero), Err(LeaseError::Uninitialized));
    assert_eq!(release(&mut state, Engine::Zero), Err(LeaseError::Uninitialized));
    assert_eq!(state.count, 2);
    assert!(!state.reset);
}

#[test]
fn acquisition_resets_only_from_zero() {
    let mut state = runtime();
    assert_eq!(acquire(&mut state, Engine::One), Ok(()));
    assert_eq!(state.count, 1);
    assert!(state.reset);
    state.reset = false;
    assert_eq!(acquire(&mut state, Engine::One), Err(LeaseError::Held));
    assert_eq!(state.count, 2);
    assert!(!state.reset);
}

#[test]
fn release_reports_a_held_remainder_and_wraps_from_zero() {
    let mut state = runtime();
    state.count = 3;
    assert_eq!(release(&mut state, Engine::One), Err(LeaseError::Held));
    assert_eq!(state.count, 2);
    state.count = 1;
    assert_eq!(release(&mut state, Engine::One), Ok(()));
    assert_eq!(state.count, 0);
    assert_eq!(release(&mut state, Engine::One), Err(LeaseError::Held));
    assert_eq!(state.count, 255);
}

#[test]
fn dispatcher_releases_only_for_value_one() {
    let mut state = runtime();
    state.count = 1;
    assert_eq!(operation_lease(&mut state, 1), 0);
    assert_eq!(state.count, 0);
    assert_eq!(operation_lease(&mut state, 0), 0);
    assert_eq!(state.count, 1);
    assert!(state.reset);
    assert_eq!(operation_lease(&mut state, 2), -12);
    assert_eq!(state.count, 2);
}
