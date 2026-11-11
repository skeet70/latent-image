use super::*;
use crate::callback_registry::{CallbackError, CallbackWord};

unsafe extern "C" fn host_tick() {}

#[test]
fn the_executor_tick_is_admitted_after_the_owned_entry_registers_it() {
    // `xm1_power_executor_timer_start_owned` registers the tick through the leaf's entry and then
    // calls `xm1_power_executor_timer_start`, whose first act is this admission.
    let word = CallbackWord(host_tick as *const () as usize);
    assert_eq!(crate::task6_executor::admit_timer_callback(word), Err(CallbackError::Unadmitted));
    crate::task6_executor::register_executor_tick(host_tick).unwrap();
    assert_eq!(crate::task6_executor::admit_timer_callback(word), Ok(()));
}
