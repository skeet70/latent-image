#[path = "../pipeline/shared.rs"]
pub(super) mod shared;
#[path = "../pipeline/callbacks.rs"]
pub(super) mod callbacks;
#[path = "../pipeline/crop_process.rs"]
pub(super) mod crop_process;
#[path = "../pipeline/font_fdl.rs"]
pub(super) mod font_fdl;
#[path = "../pipeline/font_script_store.rs"]
pub(super) mod font_script_store;
#[path = "../pipeline/font_scripts.rs"]
pub(super) mod font_scripts;
#[path = "../pipeline/image_geometry.rs"]
pub(super) mod image_geometry;
#[path = "../pipeline/image_planes.rs"]
pub(super) mod image_planes;
#[path = "../pipeline/image_references.rs"]
pub(super) mod image_references;
#[path = "../pipeline/motion_search.rs"]
pub(super) mod motion_search;

/// Host stand-in for the physical configuration reader. The real module reads the live
/// 0x420000-byte RAM allocation, which has no host mapping; no host test enters `channel_count`.
pub(super) mod profile_calibration {
    use crate::develop::calibration_core as calibration;
    use calibration::Calibration;
    use calibration::configuration::{ConfigAddress, Configuration, ConfigurationRegion};

    static CONFIGURATION: [u8; 1] = [0];

    pub(super) fn calibration_value<T>(result: Result<T, calibration::configuration::ConfigurationError<core::convert::Infallible>>) -> T {
        match result { Ok(value) => value, Err(_) => loop { core::hint::spin_loop() } }
    }

    pub(super) unsafe fn physical_calibration() -> Calibration<'static> {
        let address = ConfigAddress(0);
        let region = ConfigurationRegion::new(address, &CONFIGURATION).unwrap();
        Calibration::new(Configuration::new(region, address))
    }
}

#[cfg(test)]
mod tests;
