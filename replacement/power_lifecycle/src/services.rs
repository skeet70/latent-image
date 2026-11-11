#![cfg_attr(not(test), no_std)]

pub mod standby;
pub mod area2;
pub mod area2_codec;
pub mod resource_route;
pub mod resource_requested_route;
#[cfg(target_arch = "arm")]
pub mod osal_delay_primitives;
#[cfg(target_arch = "arm")]
pub mod osal_task_priority;
pub mod settings_shutdown_parser;
pub mod software_delay_channel;
pub mod storage_shutdown_load;
pub mod storage_shutdown_mapping;
#[cfg(target_arch = "arm")]
pub mod settings_shutdown_transfer;
#[cfg(test)]
#[path = "settings_shutdown_tests.rs"]
mod settings_shutdown_tests;
pub mod software_timer_service;
pub mod shutdown_task7_8_transport;
pub mod resource_registry;
pub mod osal;
pub mod primitives;
pub mod serial_driver;
pub mod diagnostics;
pub mod charge_timers;
pub mod queue_construction;
pub mod hardware_timer_registry;
pub mod timer_abi;
pub mod callback_registry;
pub mod shutdown;
pub mod shutdown_pins;
pub mod shutdown_rtos;
pub mod shutdown_receive;
pub mod shutdown_requests;
pub mod shutdown_completion;
pub mod shutdown_start;
pub mod device_timer_release;
pub mod task6_executor;
pub mod shutdown_task7_8;
pub mod settings_shutdown_actor;
pub mod settings_shutdown_bounds;
pub mod storage_shutdown_actor;
pub mod storage_shutdown_native;
pub mod storage_shutdown_callbacks;
pub mod storage_shutdown_defaults;
pub mod storage_shutdown_dma;
pub mod storage_shutdown_lens_control;
pub mod storage_shutdown_lens_machine;
pub mod storage_shutdown_lens_ring;
pub mod storage_shutdown_lens_service;
pub mod storage_shutdown_medium;
pub mod storage_shutdown_lifecycle;
pub mod storage_shutdown_pages;
#[cfg(target_arch = "arm")]
pub mod settings_shutdown_native;
#[cfg(target_arch = "arm")]
#[path = "../../application_nand/src/lib.rs"]
pub mod application_nand;
#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("native.S"), options(raw));
#[cfg(power_native)]
mod native_area2;

pub extern crate record_store;
pub use xm1_settings_storage_core::mirror;

#[cfg(test)]
extern crate std;
