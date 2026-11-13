//! The contrast measurement's motion layer: the lens drive a measurement run follows, where its
//! focus samples fall, how its frames are exposed, and the state the run's preparation leaves for
//! the measurement pipeline.

pub mod access;
pub mod drive;
pub mod exposure;
pub mod layout;
pub mod measured;
pub mod plan;
pub mod ports;
pub mod prepare;
pub mod preparation;
pub mod queries;
pub mod readings;
pub mod sampling;
pub mod search;
pub mod state;
pub mod statistics;

#[cfg(test)]
mod prepare_tests;
#[cfg(test)]
mod drive_tests;
#[cfg(test)]
mod exposure_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod fakes;
#[cfg(test)]
mod measured_tests;
#[cfg(test)]
mod preparation_tests;
#[cfg(test)]
mod queries_tests;
#[cfg(test)]
mod readings_tests;
#[cfg(test)]
mod sampling_tests;
#[cfg(test)]
mod world;
