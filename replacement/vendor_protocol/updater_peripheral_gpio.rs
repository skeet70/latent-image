#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerStatus(i32);
impl TimerStatus { pub(crate) const fn new(value: i32) -> Self { Self(value) } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioError { Busy, Attach(TimerStatus) }

use xm1_power_services::callback_registry::COMPLETION_SHAPE;

#[cfg(any(target_arch = "arm", test))]
extern "C" {
    /// The Power Services leaf's owned-callback entry: `shape` 0 is `extern "C" fn()`. It returns
    /// 0 when the word is registered and -255 when the leaf refuses it.
    fn xm1_power_owned_callback_register(word: u32, shape: u32) -> i32;
}

/// Registers the continuous detection handler so the Power detection entry resolves it.
#[cfg(any(target_arch = "arm", test))]
pub(crate) unsafe fn register_completion(word: u32) -> Result<(), TimerStatus> {
    let status = unsafe { xm1_power_owned_callback_register(word, COMPLETION_SHAPE) };
    if status == 0 { Ok(()) } else { Err(TimerStatus(status)) }
}

#[cfg(target_arch = "arm")]
pub use native::GpioCompletion;

#[cfg(target_arch = "arm")]
mod native {
    use super::{register_completion, GpioError, TimerStatus};
    use core::sync::atomic::{AtomicBool, Ordering};

    unsafe extern "C" {
        fn xm1_power_detection_set_mode(mode: u32) -> i32;
        fn xm1_power_detection_output_continuous(mode: u32, callback: u32) -> i32;
    }
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static NOTIFIED: AtomicBool = AtomicBool::new(false);

    pub(crate) struct PeripheralInterrupt;
    pub struct GpioCompletion { armed: bool }
    impl GpioCompletion {
        /// # Safety
        /// The caller exclusively owns the canonical continuous detection output registration.
        pub unsafe fn acquire() -> Result<Self, GpioError> {
            if ACTIVE.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return Err(GpioError::Busy); }
            NOTIFIED.store(false, Ordering::Release);
            Ok(Self { armed: false })
        }
        pub(crate) fn take_interrupt(&mut self) -> Option<PeripheralInterrupt> {
            NOTIFIED.swap(false, Ordering::AcqRel).then_some(PeripheralInterrupt)
        }
        pub(crate) fn disarm(&mut self) {
            if self.armed {
                let _ = unsafe { xm1_power_detection_output_continuous(1, 0) };
                self.armed = false;
            }
            NOTIFIED.store(false, Ordering::Release);
        }
        pub(crate) fn arm(&mut self) -> Result<(), GpioError> {
            self.disarm();
            let status = unsafe { xm1_power_detection_set_mode(0) };
            if status != 0 { return Err(GpioError::Attach(TimerStatus(status))); }
            let handler = completed as *const () as u32;
            if let Err(status) = unsafe { register_completion(handler) } { return Err(GpioError::Attach(status)); }
            let status = unsafe { xm1_power_detection_output_continuous(1, handler) };
            if status != 0 { return Err(GpioError::Attach(TimerStatus(status))); }
            self.armed = true;
            Ok(())
        }
    }
    impl Drop for GpioCompletion {
        fn drop(&mut self) { self.disarm(); ACTIVE.store(false, Ordering::Release); }
    }
    unsafe extern "C" fn completed() { NOTIFIED.store(true, Ordering::Release); }
}

#[cfg(test)]
#[path = "updater_peripheral_gpio_tests.rs"]
mod tests;
