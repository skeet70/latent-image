//! saclac's adjustment layer: the profile, scene and setting readings its modules and the
//! exposure engine decide by.

pub mod configuration;
pub mod focus_profile;
pub mod focus_reuse;
pub mod lens;
pub mod rate_policy;
pub mod scene_policy;
pub mod search_targets;

#[cfg(test)]
mod configuration_tests;
#[cfg(test)]
pub(crate) mod fakes;
#[cfg(test)]
mod focus_profile_tests;
#[cfg(test)]
mod focus_reuse_tests;
#[cfg(test)]
mod rate_policy_tests;
#[cfg(test)]
mod scene_policy_tests;
#[cfg(test)]
mod search_targets_tests;
