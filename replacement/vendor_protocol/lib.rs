#![cfg_attr(not(test),no_std)]

#[path = "../application_nand/src/lib.rs"]
pub mod application_nand;
pub mod af_capture_actor;
pub mod af_configuration_runtime;
pub mod backend;
pub mod backend_adapter;
pub mod commands;
pub mod configuration_actions_high;
pub mod configuration_actions_imaging;
pub mod configuration_actions_low;
pub mod configuration_actions_middle;
pub mod configuration_diagnostic_actor;
pub mod configuration_diagnostic_registers;
pub mod configuration_display_actors;
pub mod configuration_display_pattern;
pub mod configuration_i2c_actor;
pub mod configuration_pattern_actor;
pub mod configuration_queue;
pub mod configuration_rules;
pub mod configuration_storage;
pub mod controller;
pub mod controller_registration;
pub mod controller_root;
pub mod controller_runtime;
pub mod debug_events;
pub mod eeprom_detection;
pub mod eeprom_profile_load;
pub mod eeprom_profile_medium;
pub mod eeprom_task100;
pub mod extra_configuration;
pub mod filesystem_capacity;
pub mod filesystem_catalog;
pub mod filesystem_exfat;
pub mod filesystem_file_cpu;
pub mod filesystem_format;
pub mod filesystem_ftl;
pub mod filesystem_runtime;
pub mod filesystem_service;
pub mod imaging_driver66;
pub mod lens_actuator_cpu;
pub mod lens_control_runtime;
pub mod lens_queue_runtime;
pub mod media_parser;
pub mod metadata;
pub mod metadata_calibration;
pub mod metadata_math;
pub mod metadata_selection;
pub mod nand_event_mode;
pub mod persistent_queue;
pub mod pictbridge_runtime;
pub mod playback_aux_machines;
pub mod playback_consumer;
pub mod playback_source_workspace;
pub mod ptp_application;
pub mod ptp_runtime;
pub mod ptp_types;
pub mod result_publication;
pub mod sd_controller;
pub mod serial_program;
pub mod service;
pub mod session;
pub mod session_abi;
pub mod settings_reset;
pub mod startup;
pub mod startup_calibration;
pub mod startup_output_cache;
pub mod startup_projection;
pub mod startup_radial_geometry;
pub mod startup_radial_tables;
pub mod startup_refresh;
pub mod startup_workers;
pub mod transport;
pub mod transport_connection_cpu;
pub mod transport_start_cpu;
pub mod updater_peripheral;
pub mod updater_serial;
pub mod upload_runtime;
pub mod upload_transaction;
pub mod vendor_display_runtime;
pub mod vendor_wireless_access_point;
pub mod vendor_wireless_access_point_authenticator;
pub mod vendor_wireless_actor;
pub mod vendor_wireless_adapter;
pub mod vendor_wireless_boot;
pub mod vendor_wireless_boot_detach;
pub mod vendor_wireless_boot_objects;
pub mod vendor_wireless_boot_query;
pub mod vendor_wireless_boot_query_driver;
pub mod vendor_wireless_boot_runtime;
pub mod vendor_wireless_lifecycle;
pub mod vendor_wireless_lifecycle_download;
pub mod vendor_wireless_lifecycle_firmware;
pub mod vendor_wireless_lifecycle_runtime;
pub mod vendor_wireless_lifecycle_sdio;
pub mod vendor_wireless_network;
pub mod vendor_wireless_parameter_driver;
pub mod vendor_wireless_peers;
pub mod vendor_wireless_power;
pub mod vendor_wireless_radio_program;
pub mod vendor_wireless_secondary;
pub mod vendor_wireless_service;
pub mod vendor_wireless_service_host;
pub mod vendor_wireless_service_task;
pub mod vendor_wireless_station_driver;
pub mod wireless_association;
pub mod wireless_network_services;

#[cfg(vendor_protocol_native)]
mod native_probe;

#[cfg(all(target_arch = "arm", vendor_protocol_firmware))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod session_abi_tests;
#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod backend_tests;
#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod eeprom_profile_medium_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod upload_service_tests;
#[cfg(test)]
mod filesystem_catalog_tests;
#[cfg(test)]
mod ptp_application_tests;
#[cfg(test)]
mod ptp_runtime_tests;
#[cfg(test)]
mod vendor_wireless_service_task_tests;
