//! Stock timer entries. The registry defines them; the executors call them through this one declaration.

use crate::hardware_timer_registry::HardwareTimerRequest;

extern "C" {
    pub fn xm1_power_hardware_timer_acquire(selector: u32, output: *mut u32) -> i32;
    pub fn xm1_power_hardware_timer_start(handle: u32, request: *const HardwareTimerRequest) -> i32;
    pub fn xm1_power_hardware_timer_stop(handle: u32) -> i32;
    pub fn xm1_power_hardware_timer_release(handle: u32) -> i32;
}
