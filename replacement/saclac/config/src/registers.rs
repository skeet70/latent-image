#![forbid(unsafe_code)]

//! Packing a configuration into the pipeline's register snapshot and restoring the hardware from
//! it, by the groups a flag word selects (0xda21cc, 0xda2354).

use xm1_saclac_shared::image_registers::{pack, RegisterGroup, RegisterSnapshot};

/// Group 0 when any of bits 17..=30 is set, group `n` (1..=14) for bit `16 + n`.
pub fn groups(flags: u32) -> impl Iterator<Item = RegisterGroup> {
    RegisterGroup::ALL.into_iter().filter(move |group| match group.index() {
        0 => flags & 0x7ffe_0000 != 0,
        index => flags & 1 << (16 + index) != 0,
    })
}

/// 0xda21cc.
pub fn pack_groups<S: RegisterSnapshot>(flags: u32, configuration: S::Configuration, snapshot: &mut S) {
    for group in groups(flags) {
        pack(group, configuration, snapshot);
    }
}

/// The pipeline hardware grant the restore holds (0xda1b4c, 0xda1d48), which stays stock with
/// the pipeline driver.
pub trait PipelineHardware {
    fn acquire(&mut self, client: u32) -> u32;
    fn release(&mut self, client: u32, grant: u32);
}

/// The client the restore acquires the hardware as.
pub const RESTORE_CLIENT: u32 = 0x1000;

/// Restores the selected groups while holding the hardware (0xda2354).
pub fn restore_groups(flags: u32, snapshot: &mut impl RegisterSnapshot, hardware: &mut impl PipelineHardware) {
    let grant = hardware.acquire(RESTORE_CLIENT);
    for group in groups(flags) {
        snapshot.restore(group);
    }
    hardware.release(RESTORE_CLIENT, grant);
}
