//! The contrast measurement's motion layer: the lens drive a measurement run follows, where its
//! focus samples fall, and the state the run's preparation leaves for the measurement pipeline.

pub mod drive;
pub mod ports;
pub mod queries;
pub mod sampling;
pub mod state;

#[cfg(test)]
mod drive_tests;
#[cfg(test)]
mod fakes;
#[cfg(test)]
mod queries_tests;
#[cfg(test)]
mod sampling_tests;
