//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x006ac16c and 0x006aa1fc.

use crate::backend::{ConfigImages, ConfigSpan};
use xm1_power_services::storage_shutdown_actor::{MediumStatus, NandBlock};
use xm1_power_services::storage_shutdown_defaults::{self, DefaultStorageIo};
use xm1_power_services::storage_shutdown_load::DefaultBank;
use xm1_power_services::storage_shutdown_medium::{NandSoftware, StorageShutdownMedium};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSlot(u8);
impl ProfileSlot {
    pub const fn new(slot: u8) -> Option<Self> {
        if slot < 20 { Some(Self(slot)) } else { None }
    }
}

pub struct ProfileMedium<N, D> {
    pub medium: StorageShutdownMedium<N>,
    defaults: D,
    first: NandBlock,
}
impl<N: NandSoftware, D: DefaultStorageIo> ProfileMedium<N, D> {
    pub const fn new(medium: StorageShutdownMedium<N>, defaults: D, partition: NandBlock) -> Self {
        Self { medium, defaults, first: NandBlock(partition.0.wrapping_sub(0x380)) }
    }

    pub fn read_profile(&mut self, slot: ProfileSlot, configuration: &mut ConfigImages<'_>) -> MediumStatus {
        let span = match ConfigSpan::new(0x3e0000, 0x40000) {
            Ok(span) => span,
            Err(_) => return MediumStatus(-255),
        };
        let destination = match configuration.encoded_region_mut(span) {
            Ok(destination) => destination,
            Err(_) => return MediumStatus(-255),
        };
        let destination: &mut [u8; 0x40000] = match destination.try_into() {
            Ok(destination) => destination,
            Err(_) => return MediumStatus(-255),
        };
        self.medium.read_profile(NandBlock(self.first.0.wrapping_add(u32::from(slot.0) * 2)), destination)
    }

    pub fn read_defaults(&mut self, bank: DefaultBank, configuration: &mut ConfigImages<'_>) -> MediumStatus {
        let (offset, length) = match bank {
            DefaultBank::Primary => (0x3e0000, 0x2000),
            DefaultBank::Extended => (0x3e2000, 0x15e00),
        };
        let span = match ConfigSpan::new(offset, length) {
            Ok(span) => span,
            Err(_) => return MediumStatus(-255),
        };
        let destination = match configuration.encoded_region_mut(span) {
            Ok(destination) => destination,
            Err(_) => return MediumStatus(-255),
        };
        storage_shutdown_defaults::read_defaults(&mut self.defaults, bank, destination)
    }
}

#[cfg(target_arch = "arm")]
pub type NativeProfileMedium = ProfileMedium<
    xm1_power_services::storage_shutdown_native::NativeNand,
    xm1_power_services::storage_shutdown_defaults::native::NativeDefaults,
>;

#[cfg(target_arch = "arm")]
pub fn native(partition: NandBlock) -> NativeProfileMedium {
    ProfileMedium::new(
        xm1_power_services::storage_shutdown_native::medium(),
        xm1_power_services::storage_shutdown_defaults::native::NativeDefaults,
        partition,
    )
}
