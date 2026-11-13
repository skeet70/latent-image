//! saclac's adjustment layer: the profile, scene and setting readings its modules and the
//! exposure engine decide by.

pub mod focus_profile;
pub mod focus_reuse;
pub mod scene_policy;

#[cfg(test)]
mod focus_profile_tests;
#[cfg(test)]
mod focus_reuse_tests;
#[cfg(test)]
mod scene_policy_tests;
