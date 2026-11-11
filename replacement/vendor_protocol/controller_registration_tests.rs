use super::*;
use xm1_power_services::callback_registry::{registry, CallbackWord};

// The four TimerCallbackEntries producers' guest addresses.
const PRODUCERS: [u32; 4] = [0x0094_0000, 0x0094_0004, 0x0094_0008, 0x0094_000c];

#[test]
fn the_four_producers_register_before_the_timer_entry_resolves_them() {
    for word in PRODUCERS {
        let word = CallbackWord(word as usize);
        assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).unwrap_err().status(), -255);
        assert_eq!(unsafe { register_timer_callback(word.0 as u32) }, Ok(()));
        assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).map(|callback| callback as usize), Ok(word.0));
    }
}

#[test]
fn a_refused_producer_word_reports_the_entry_status() {
    // 0x0094_0001 is unaligned, which the entry refuses.
    assert_eq!(unsafe { register_timer_callback(0x0094_0001) }, Err(ServiceStatus(-255)));
}
