//! The owned X-M1 1.50 menu engine (#128, `reports/owned-menu-engine-design.md`):
//! a pure transition over the typed engine block for the event classes listed
//! in `engine`.

#![allow(unexpected_cfgs, unused_attributes)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), allow(dead_code))]

pub mod block;
pub mod engine;
pub mod frame;
pub mod state;
pub mod tree;

#[cfg(test)]
mod tests;

#[cfg(all(test, rb_menu_engine_cases))]
mod owner;
