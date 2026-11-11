use super::*;
use xm1_power_services::callback_registry::{registry, CallbackWord};

#[test]
fn the_completion_registers_before_the_detection_entry_resolves_it() {
    let word = CallbackWord(0x0095_0000);
    assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).unwrap_err().status(), -255);
    assert_eq!(unsafe { register_completion(word.0 as u32) }, Ok(()));
    assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).map(|callback| callback as usize), Ok(word.0));
}

#[test]
fn a_refused_completion_word_reports_the_entry_status() {
    assert_eq!(unsafe { register_completion(0x0095_0001) }, Err(TimerStatus(-255)));
}
