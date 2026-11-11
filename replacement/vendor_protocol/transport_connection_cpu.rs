use crate::filesystem_file_cpu::{CloseCompletion, FileCpu, FileError, FileHandle};
use crate::filesystem_runtime::SectorIo;
use crate::transport::PublishedRegion;
use crate::transport_start_cpu::UsbStart;
use crate::commands::ControllerState;
use crate::controller::RequestKind;
use crate::ptp_runtime::PtpRuntime;
use crate::ptp_types::BatteryLevel;
use crate::pictbridge_runtime::PictBridge;
use crate::controller_registration::{ControllerRegistrations, TimerRegistry};
use xm1_power_services::primitives::ServiceStatus;
use xm1_power_services::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[cfg(test)]
#[path = "transport_connection_cpu_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionNotification { Connected, Disconnected }
impl ConnectionNotification {
    pub const fn status(self) -> i32 {
        match self { Self::Connected => -12, Self::Disconnected => -11 }
    }
}

/// Closes the handle stock submits through 0x0071c938.
pub trait ActiveFileClose {
    fn close(&mut self, handle: FileHandle) -> Result<CloseCompletion, FileError>;
}
pub struct FileClose<'a, S> { pub files: &'a mut FileCpu, pub sectors: &'a mut S }
impl<S: SectorIo> ActiveFileClose for FileClose<'_, S> {
    fn close(&mut self, handle: FileHandle) -> Result<CloseCompletion, FileError> { self.files.close(self.sectors, handle) }
}

pub struct ConnectionState {
    active: Option<FileHandle>,
    selected: Option<PublishedRegion>,
    connected: bool,
    absent_ticks: u32,
}
impl ConnectionState {
    pub const fn new() -> Self {
        Self { active: None, selected: None, connected: false, absent_ticks: 0 }
    }
    pub fn set_active(&mut self, handle: FileHandle) { self.active = Some(handle); }
    pub const fn active(&self) -> Option<FileHandle> { self.active }
    pub const fn secondary_active(&self) -> bool { self.connected }
    pub fn clear_secondary_activity(&mut self) { self.connected = false; }
    pub fn select_request_buffer(&mut self, kind: RequestKind, protocol: PublishedRegion, service: PublishedRegion, unselected: PublishedRegion) -> PublishedRegion {
        let region = match kind {
            RequestKind::Protocol => protocol, RequestKind::Service => service,
            RequestKind::Unselected | RequestKind::FileOnly => unselected,
        };
        self.selected = Some(region);
        region
    }
    pub const fn selected_buffer(&self) -> Option<PublishedRegion> { self.selected }
    /// Stock 0x00d82210 for request handle 2: +0x404 slot 4, then +0x408 slots 0 and 4.
    pub fn begin_connection_protocol(
        &mut self, ptp: &mut PtpRuntime, bridge: &mut PictBridge,
        registrations: &mut ControllerRegistrations<'_>, registry: &mut impl TimerRegistry,
        current: BatteryLevel,
    ) -> Result<(), ServiceStatus> {
        ptp.reset();
        bridge.reset_flags();
        bridge.initialize();
        let short = registrations.start_short(registry);
        ptp.set_battery(current);
        let long = registrations.start_long(registry);
        short.and(long)
    }
    pub fn battery_changed(&mut self, ptp: &mut PtpRuntime, current: BatteryLevel) {
        if ptp.battery() == current { return; }
        ptp.set_battery(current);
        ptp.notify(0);
    }
    /// Stock 0x00d81f18.
    pub fn update_connection(&mut self, state_eleven: bool, registrations: &mut ControllerRegistrations<'_>) -> Option<ConnectionNotification> {
        let notification = if !self.connected {
            if state_eleven {
                self.connected = true;
                self.absent_ticks = 0;
                Some(ConnectionNotification::Connected)
            } else { None }
        } else if state_eleven {
            self.absent_ticks = 0;
            None
        } else {
            self.absent_ticks = self.absent_ticks.wrapping_add(1);
            if self.absent_ticks > 9 {
                self.connected = false;
                self.absent_ticks = 0;
                Some(ConnectionNotification::Disconnected)
            } else { None }
        };
        registrations.acknowledge_secondary_poll();
        notification
    }
    pub fn take_close(&mut self) -> Option<FileHandle> { self.active.take() }
    pub fn close_active(
        &mut self, phase: ControllerState, usb: &mut UsbStart<'_>,
        files: &mut impl ActiveFileClose, rtos: &mut impl ShutdownRtos,
    ) -> Result<(), CloseError> {
        let Some(handle) = self.active else { return Ok(()); };
        let started = if matches!(phase, ControllerState::State8 | ControllerState::State9 | ControllerState::ActiveVendor | ControllerState::PendingVendor) {
            usb.clear_active();
            publish_close_started(rtos)
        } else { Ok(()) };
        let result = files.close(handle);
        self.active = None;
        let status = match &result { Ok(completion) => completion.status, Err(error) => error.status() };
        let completed = publish_close_status(status, rtos);
        started.map_err(CloseError::Publication)?;
        completed.map_err(CloseError::Publication)?;
        result.map(|_| ()).map_err(CloseError::File)
    }
    pub fn complete_close(&mut self, completion: CloseCompletion, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
        self.active = None;
        publish_close_status(completion.status, rtos)
    }
}

pub fn publish_close_started(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    post(0x3407, &[0; 24], rtos)
}
pub fn publish_close_status(status: u32, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let mut payload = [0; 24];
    payload[..4].copy_from_slice(&11u32.to_le_bytes());
    payload[4..8].copy_from_slice(&u32::from(status != 0).to_le_bytes());
    post(0x3408, &payload, rtos)
}
fn post(operation: u16, payload: &[u8; 24], rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(operation.into()), sender, destination: QueueId(36), payload })
}

#[derive(Debug)]
pub enum CloseError { File(FileError), Publication(KernelStatus) }
