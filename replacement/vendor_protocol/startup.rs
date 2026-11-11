use crate::backend::{BackendError, ConfigImages};
use crate::startup_projection::{StartupProjection, StartupProjectionError};
use xm1_settings_storage_core::mirror::{BackingRange, BackingStatus, HeaderBackupDriver, SharedBacking, BACKING_BYTES};

pub const WORKSPACE_BYTES: usize = 0x2740000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider { ConfigurationWrite, ConfigurationRead, ConfigurationBounds, Payload }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationCapacity(u32);
impl ConfigurationCapacity { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderRegistration { pub providers: [Provider;4], pub capacity: ConfigurationCapacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbMode { Vendor }
impl UsbMode { pub const fn get(self) -> u32 { match self { Self::Vendor=>3 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbObserver { UsbManager }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsbStartRequest { pub mode: UsbMode, pub observer: UsbObserver, pub providers: ProviderRegistration }
impl UsbStartRequest { pub const fn event_code(self) -> u16 { 0x3403 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupError { Backend(BackendError), Projection(StartupProjectionError) }
impl From<BackendError> for StartupError {
    fn from(error: BackendError) -> Self { Self::Backend(error) }
}
impl From<StartupProjectionError> for StartupError {
    fn from(error: StartupProjectionError) -> Self { Self::Projection(error) }
}

pub struct ServiceStartup<'a> {
    workspace: &'a mut [u8],
    providers: Option<ProviderRegistration>,
    profile_flag: Option<bool>,
    request: Option<UsbStartRequest>,
}
impl<'a> ServiceStartup<'a> {
    pub fn adopt(workspace: &'a mut [u8]) -> Result<Self, BackendError> {
        if workspace.len() < WORKSPACE_BYTES { return Err(BackendError::Capacity); }
        Ok(Self { workspace, providers: None, profile_flag: None, request: None })
    }
    pub fn providers(&self) -> Option<ProviderRegistration> { self.providers }
    pub fn profile_flag(&self) -> Option<bool> { self.profile_flag }
    pub fn workspace(&self) -> &[u8] { &self.workspace[..WORKSPACE_BYTES] }
    pub fn take_usb_request(&mut self) -> Option<UsbStartRequest> { self.request.take() }
    pub fn start_1705<D: HeaderBackupDriver>(&mut self, configuration: &mut ConfigImages<'_>, projection: &mut StartupProjection, backing: &mut SharedBacking<'_,D>, export: &mut [u8;BACKING_BYTES]) -> Result<UsbStartRequest, StartupError> {
        if self.request.is_some() { return Err(BackendError::Unavailable.into()); }
        let registration = ProviderRegistration { providers: [Provider::ConfigurationWrite,Provider::ConfigurationRead,Provider::ConfigurationBounds,Provider::Payload], capacity: ConfigurationCapacity(0x420000) };
        self.providers = Some(registration);
        self.workspace[..WORKSPACE_BYTES].fill(0xff);
        let status = backing.export(BackingRange::new(0,BACKING_BYTES as u32),export);
        match status {
            BackingStatus::Success => {},
            BackingStatus::Refused(status) => return Err(BackendError::Medium(status.0).into()),
            _ => return Err(BackendError::Capacity.into()),
        }
        self.profile_flag = Some(export[0x5e] & 0x80 != 0);
        let calibration = configuration.byte(0x238).ok_or(BackendError::Capacity)?;
        if calibration == 1 { projection.prepare_output(configuration, true)?; }
        let request = UsbStartRequest { mode:UsbMode::Vendor,observer:UsbObserver::UsbManager,providers:registration };
        self.request = Some(request);
        Ok(request)
    }
}
