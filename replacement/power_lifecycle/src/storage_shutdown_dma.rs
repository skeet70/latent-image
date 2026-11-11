#![cfg(target_arch = "arm")]

use crate::shutdown::KernelStatus;

extern "C" {
    fn xm1_power_detection_set_mode(mode: u32) -> i32;
    fn xm1_power_detection_output_continuous(mode: u32, callback: u32) -> i32;
}

fn result(status: i32) -> Result<(), KernelStatus> {
    if status == 0 { Ok(()) } else { Err(KernelStatus(status)) }
}

pub unsafe fn prepare_service() -> Result<(), KernelStatus> {
    result(unsafe { xm1_power_detection_set_mode(0) })
}

pub unsafe fn register_callback(enabled: bool) -> Result<(), KernelStatus> {
    let callback = if enabled { crate::storage_shutdown_callbacks::xm1_storage_shutdown_dma as *const () as u32 } else { 0 };
    result(unsafe { xm1_power_detection_output_continuous(1, callback) })
}
