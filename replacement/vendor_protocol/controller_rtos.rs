//! RTOS operations the controller issues outside the queue transport.

use xm1_power_services::shutdown::{KernelStatus, ShutdownRtos, TaskId};

/// 0x00723a14: interface slot +0x1c through table 0x007cad9c, error class 5.
pub trait ControllerRtos: ShutdownRtos {
    fn set_task_priority(&mut self, task: TaskId, priority: u32) -> Result<(), KernelStatus>;
}

#[cfg(target_arch = "arm")]
impl ControllerRtos for xm1_power_services::shutdown_rtos::NativeShutdownRtos {
    fn set_task_priority(&mut self, task: TaskId, priority: u32) -> Result<(), KernelStatus> {
        extern "C" { fn xm1_power_set_task_phase(task: u32, priority: u32) -> i32; }
        let status = unsafe { xm1_power_set_task_phase(u32::from(task.0), priority) };
        if status == 0 { Ok(()) } else { Err(KernelStatus(status)) }
    }
}
