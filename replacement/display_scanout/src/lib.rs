#![no_std]

pub mod commit;
pub mod scheduler;
pub mod timing;
mod timing_keys;
pub mod shadow;
pub mod panel;
pub mod panel_actor;
pub mod panel_device;
pub mod panel_formatter;
pub mod panel_route;
pub mod completion_queue;
#[cfg(test)]
mod completion_queue_tests;
pub mod bootstrap;
pub mod mode_frontends;
pub mod operation_lease;
#[cfg(test)]
mod bootstrap_tests;
#[cfg(test)]
mod mode_frontends_tests;
#[cfg(test)]
mod operation_lease_tests;
pub use commit::EngineState;
#[cfg(target_arch = "arm")]
mod abi;
#[cfg(target_arch = "arm")]
mod layout;
#[cfg(target_arch = "arm")]
mod publication_edge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine { Zero, One }
impl Engine {
    pub const fn index(self) -> usize { match self { Self::Zero => 0, Self::One => 1 } }
    pub const fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Zero), 1 => Some(Self::One), _ => None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank { Scanout, Timing, Formatter }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register { pub engine: Engine, pub bank: Bank, pub offset: u16 }
pub trait Hardware {
    fn read(&mut self, register: Register) -> u32;
    fn write(&mut self, register: Register, value: u32);
    fn enter_critical(&mut self) -> u32;
    fn leave_critical(&mut self, token: u32);
    fn yield_zero(&mut self);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Uninitialized, InvalidArgument, Capacity }
impl Error { pub const fn stock(self) -> i32 { match self { Self::Uninitialized => -10, Self::InvalidArgument => -30, Self::Capacity => -20 } } }

#[cfg(all(target_arch = "arm", not(feature = "library")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
#[cfg(test)]
extern crate std;
#[cfg(test)]
mod shadow_tests;
#[cfg(test)]
mod scheduler_tests;
#[cfg(test)]
mod timing_tests;
#[cfg(test)]
mod commit_tests;
#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod panel_actor_tests;
#[cfg(test)]
mod panel_device_tests;
#[cfg(test)]
mod panel_formatter_tests;
#[cfg(test)]
mod panel_route_tests;
