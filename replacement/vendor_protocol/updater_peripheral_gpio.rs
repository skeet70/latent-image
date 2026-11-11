#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerStatus(i32);
impl TimerStatus { pub(crate) const fn new(value: i32) -> Self { Self(value) } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioError { Busy, Attach(TimerStatus) }

#[cfg(target_arch = "arm")]
pub use native::GpioCompletion;

#[cfg(target_arch = "arm")]
mod native {
    use super::{GpioError, TimerStatus};
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
            let status = unsafe { xm1_power_detection_output_continuous(1, completed as *const () as u32) };
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
