#[path = "../rtos_scheduler/src/s3d.rs"]
pub mod s3d;

#[cfg(target_arch = "arm")]
#[path = "../rtos_scheduler/src/s3d_cpu.rs"]
pub mod cpu;
