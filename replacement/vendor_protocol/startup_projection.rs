//! X-M1 1.50 0x772e70 and 0x72c844, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::backend::{BackendError, ConfigImages};
use crate::metadata_calibration::{MetadataCalibration, RegionError};
use crate::startup_calibration::{StartupCalibration, StartupCalibrationError, StartupCalibrationMode};
use crate::startup_output_cache::{ConfigurationLookupCache, StartupOutputCache};
use crate::startup_refresh::StartupRefresh;
use crate::startup_workers::StartupWorkers;
use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationRegion};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupProjectionError {
    Backend(BackendError),
    Configuration(RegionError),
    Calibration(StartupCalibrationError),
}
impl From<BackendError> for StartupProjectionError {
    fn from(error: BackendError) -> Self { Self::Backend(error) }
}
impl From<RegionError> for StartupProjectionError {
    fn from(error: RegionError) -> Self { Self::Configuration(error) }
}
impl From<StartupCalibrationError> for StartupProjectionError {
    fn from(error: StartupCalibrationError) -> Self { Self::Calibration(error) }
}

pub struct StartupProjection {
    calibration: StartupCalibration,
    refresh: StartupRefresh,
    lookup: ConfigurationLookupCache,
    workers: StartupWorkers,
}
impl StartupProjection {
    pub const fn new() -> Self {
        Self { calibration: StartupCalibration::new(), refresh: StartupRefresh::new(), lookup: ConfigurationLookupCache::new(), workers: StartupWorkers::new() }
    }
    pub fn calibration(&self) -> &StartupCalibration { &self.calibration }
    pub fn refresh(&self) -> &StartupRefresh { &self.refresh }
    pub fn lookup(&mut self) -> &mut ConfigurationLookupCache { &mut self.lookup }
    pub fn workers(&self) -> &StartupWorkers { &self.workers }

    pub fn prepare_output(&mut self, configuration: &mut ConfigImages<'_>, unregistered: bool) -> Result<(), StartupProjectionError> {
        let mode = if unregistered { StartupCalibrationMode::Recompute } else { StartupCalibrationMode::Reload };
        self.calibration.prepare(&mut metadata(configuration)?, mode)?;
        self.calibration.persist(configuration)?;
        self.refresh.refresh(&mut metadata(configuration)?)?;
        StartupOutputCache::rebuild(configuration, &mut self.lookup)?;
        self.workers.prepare_registered(configuration)?;
        if unregistered { self.workers.initialize(configuration)?; }
        Ok(())
    }

    pub fn publish_configuration(&mut self, configuration: &ConfigImages<'_>) -> Result<(), StartupProjectionError> {
        self.calibration.rebuild_keys(&mut metadata(configuration)?)?;
        Ok(())
    }
}
impl Default for StartupProjection {
    fn default() -> Self { Self::new() }
}

fn metadata<'a>(configuration: &'a ConfigImages<'_>) -> Result<MetadataCalibration<'a>, RegionError> {
    let region = ConfigurationRegion::new(ConfigAddress(0), configuration.encoded_bytes())?;
    Ok(MetadataCalibration::new(Configuration::new(region, ConfigAddress(0))))
}

#[cfg(test)]
#[path = "startup_projection_tests.rs"]
mod tests;
