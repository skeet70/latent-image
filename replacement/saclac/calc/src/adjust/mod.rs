//! saclac's adjustment layer: the profile, scene and setting readings its modules and the
//! exposure engine decide by.

pub mod configuration;
pub mod focus_profile;
pub mod focus_reuse;
pub mod lens;
pub mod motion;
pub mod peak;
pub mod rate_policy;
pub mod rate_samples;
pub mod result_dispatch;
pub mod sample_grid;
pub mod scene_policy;
pub mod scene_settings;
pub mod search_targets;
pub mod shot_trace;

#[cfg(test)]
mod configuration_tests;
#[cfg(test)]
pub(crate) mod fakes;
#[cfg(test)]
mod focus_profile_tests;
#[cfg(test)]
mod focus_reuse_tests;
#[cfg(test)]
mod peak_tests;
#[cfg(test)]
mod rate_policy_tests;
#[cfg(test)]
mod rate_samples_tests;
#[cfg(test)]
mod result_dispatch_tests;
#[cfg(test)]
mod sample_grid_tests;
#[cfg(test)]
mod scene_policy_tests;
#[cfg(test)]
mod scene_settings_tests;
#[cfg(test)]
mod search_targets_tests;
#[cfg(test)]
mod shot_trace_tests;
