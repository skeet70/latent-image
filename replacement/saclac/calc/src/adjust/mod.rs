//! saclac's adjustment layer: the profile, scene and setting readings its modules and the
//! exposure engine decide by.

pub mod focus_profile;
pub mod focus_reuse;
pub mod peak;
pub mod rate_samples;
pub mod sample_grid;
pub mod scene_policy;
pub mod scene_settings;
pub mod shot_trace;

#[cfg(test)]
mod focus_profile_tests;
#[cfg(test)]
mod focus_reuse_tests;
#[cfg(test)]
mod peak_tests;
#[cfg(test)]
mod rate_samples_tests;
#[cfg(test)]
mod sample_grid_tests;
#[cfg(test)]
mod scene_policy_tests;
#[cfg(test)]
mod scene_settings_tests;
#[cfg(test)]
mod shot_trace_tests;
