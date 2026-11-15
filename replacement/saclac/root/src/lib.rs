#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's composition root. `Services` binds each module's port traits to the module that
//! implements them, and what no merged module implements to the stock entries and memory in
//! `stock`.

pub mod geometry;
pub mod controller_geometry;
pub mod controller_helpers;
pub mod controller_layers;
pub mod controller_lifecycle;
pub mod controller_profiles;
pub mod live_view_ports;
pub mod record_contexts;
pub mod root_ports;
pub mod root_utilities;
pub mod command_source;
pub mod lens_ports;
pub mod child_runtime;
pub mod live_router_ports;
pub mod s1_router_ports;
pub mod runtime;
pub mod statistics_results;
pub mod controller_source;
mod plan_services;
mod controller_replies;
mod calculation_memory;
pub mod calculation_services;
mod motion_services;
mod motion_geometry;
mod motion_memory;
mod motion_preparation;
mod motion_projection;
mod motion_plan;
pub mod completion_callbacks;
pub mod exposure_memory;
pub mod exposure_services;
pub mod shot_services;
mod shot_lens;
mod shot_records;
mod shot_pipeline_records;
mod shot_dump;
mod shot_diagnostics;
mod shot_platform;
pub mod shot_subjects;
mod shot_preparation_records;
mod shot_statistics;
mod focus_area;
mod round_image;
pub mod measurement_runtime;
mod capture_gains;
mod lens_calibration;
mod profile_calibration;
mod shot_input;
mod round_source;
mod round_auxiliary;
mod round_scene;
pub mod round_ports;
pub mod peer_context;
pub mod peer_services;
pub mod acquisition;
pub mod round_services;
mod controller_records;
pub mod controller_services;
pub mod lens;
pub mod lens_com;
pub mod records;
pub mod scene;
pub mod search_runtime;
pub mod services;
pub mod stages;
#[allow(unsafe_code)]
pub mod stock;

pub use services::Services;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod lens_com_tests;
#[cfg(test)]
mod lens_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod stages_tests;
#[cfg(test)]
mod acquisition_tests;
#[cfg(test)]
mod peer_context_tests;
#[cfg(test)]
mod round_services_tests;
#[cfg(test)]
mod shot_memory_tests;
#[cfg(test)]
mod motion_services_tests;
