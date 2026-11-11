use crate::commands::{Commands, CompletionDisposition, CompletionError, ControllerState, PreparedPayload, VendorCompletion};
use crate::pictbridge_runtime::PictBridge;
use crate::transport::{TeardownProgress, TransportError};
use xm1_power_services::{
    shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos},
    shutdown_task7_8::{self, RegistryHandle},
    shutdown_task7_8_transport::{self, RegistrationRegistry, UsbCompletionPolicy, UsbRegistrationStorage, UsbSecondaryMode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionHandle(pub u32);
/// Stock +0x3f0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestHandle(pub u32);
impl RequestHandle {
    pub const fn kind(self) -> RequestKind { RequestKind::parse(self.0) }
}
/// Handle 4 opens the file phase like handle 2 and selects no callbacks or buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind { Unselected, Protocol, Service, FileOnly }
impl RequestKind {
    pub const fn parse(raw: u32) -> Self {
        match raw { 2 => Self::Protocol, 3 => Self::Service, 4 => Self::FileOnly, _ => Self::Unselected }
    }
    /// Stock 0x00d84af8 tests handles 2 and 4.
    pub const fn opens_files(self) -> bool { matches!(self, Self::Protocol | Self::FileOnly) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerPublication {
    Completion { handle: CompletionHandle },
    Request { handle: RequestHandle },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerError { Completion(CompletionError), Transport(TransportError), Acknowledgement(KernelStatus), ShutdownInProgress }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownDisposition { Ignored, Deferred, Pending, Complete }

pub fn publish_controller_status(publication: ControllerPublication, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let (operation, handle) = match publication {
        ControllerPublication::Completion { handle } => (RequestId(0x1714), handle.0),
        ControllerPublication::Request { handle } => (RequestId(0xb403), handle.0),
    };
    let mut payload = [0; 8];
    payload[..4].copy_from_slice(&handle.to_le_bytes());
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation, sender, destination: QueueId(7), payload: &payload })
}

pub fn publish_secondary_status(status: xm1_power_services::primitives::ServiceStatus, rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    let payload = status.0.to_le_bytes();
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x171a), sender, destination: QueueId(7), payload: &payload })
}

#[cfg(target_arch = "arm")]
pub unsafe extern "C" fn secondary_status_consumer(status: xm1_power_services::primitives::ServiceStatus) {
    let _ = publish_secondary_status(status, &mut xm1_power_services::shutdown_rtos::NativeShutdownRtos);
}

/// Stock +0x404 slots 8 and 0xc.
pub trait ProtocolFiles {
    /// 0x00d90968. The chain at 0x00d90b7c..0x00d91678 posts 0x340c.
    fn start(&mut self);
    /// 0x00d8d5b0: sets word 0x0146a644.
    fn abort(&mut self);
}

/// Stock 0x00d7c790 over the open USB session.
pub trait SessionTeardown {
    fn resume(&mut self) -> Result<TeardownProgress, TransportError>;
}

pub struct ShutdownResources<'a, S, R, T, O, F> {
    pub registrations: &'a mut S,
    pub registry: &'a mut R,
    pub teardown: &'a mut T,
    pub rtos: &'a mut O,
    pub files: &'a mut F,
    pub transfer: &'a mut PictBridge,
    pub short_timer: &'a mut Option<RegistryHandle>,
    pub long_timer: &'a mut Option<RegistryHandle>,
}

#[derive(Clone, Copy)]
enum ShutdownStage { Begin, Teardown, Cleanup, Publish(ControllerPublication), Acknowledge }

pub struct QueueController {
    state: ControllerState,
    completion_handle: CompletionHandle,
    request_handle: RequestHandle,
    shutdown: Option<ShutdownStage>,
}

impl QueueController {
    pub const fn new(state: ControllerState, completion_handle: CompletionHandle, request_handle: RequestHandle) -> Self {
        Self { state, completion_handle, request_handle, shutdown: None }
    }
    pub const fn state(&self) -> ControllerState { self.state }
    pub const fn completion_handle(&self) -> CompletionHandle { self.completion_handle }
    pub const fn request_handle(&self) -> RequestHandle { self.request_handle }
    pub fn set_completion_handle(&mut self, handle: CompletionHandle) { self.completion_handle = handle; }
    pub fn set_request_handle(&mut self, handle: RequestHandle) { self.request_handle = handle; }

    pub fn transition(&mut self, state: ControllerState) -> Result<(), ControllerError> {
        if self.shutdown.is_some() { return Err(ControllerError::ShutdownInProgress); }
        self.state = state;
        Ok(())
    }

    /// Applies the 0x3408 cell of the current state. On `Deferred` the caller queues the message it parsed.
    pub fn consume_completion(
        &mut self, completion: VendorCompletion, commands: &mut Commands,
        reply: &mut [u8; 512], prepared: Option<&PreparedPayload<'_>>,
    ) -> Result<CompletionDisposition, ControllerError> {
        if self.shutdown.is_some() { return Err(ControllerError::ShutdownInProgress); }
        match self.state {
            ControllerState::ActiveVendor if self.request_handle.kind() == RequestKind::Service => return Ok(CompletionDisposition::Deferred),
            ControllerState::PendingVendor => self.state = ControllerState::ActiveVendor,
            ControllerState::ActiveVendor | ControllerState::State4 | ControllerState::State5 | ControllerState::State6 | ControllerState::State7 => {}
            _ => return Ok(CompletionDisposition::Ignored),
        }
        commands.consume_vendor_completion(completion, reply, prepared).map_err(ControllerError::Completion)
    }

    /// On `Deferred` the caller queues the 0x3401 message.
    pub fn shutdown<S: UsbRegistrationStorage, R: RegistrationRegistry, T: SessionTeardown, O: ShutdownRtos, F: ProtocolFiles>(
        &mut self, resources: &mut ShutdownResources<'_, S, R, T, O, F>,
    ) -> Result<ShutdownDisposition, ControllerError> {
        if self.shutdown.is_none() {
            match self.state {
                ControllerState::State5 => return Ok(ShutdownDisposition::Deferred),
                ControllerState::State7 => return Ok(ShutdownDisposition::Ignored),
                _ => self.shutdown = Some(ShutdownStage::Begin),
            }
        }
        loop {
            match self.shutdown {
                Some(ShutdownStage::Begin) => {
                    match self.state {
                        ControllerState::Stopped => {}
                        ControllerState::Starting => self.state = ControllerState::Stopped,
                        ControllerState::State2 | ControllerState::State3 => {
                            shutdown_task7_8_transport::unregister_primary(resources.registrations, resources.registry);
                        }
                        ControllerState::State4 => {
                            shutdown_task7_8_transport::unregister_primary(resources.registrations, resources.registry);
                            resources.files.abort();
                            self.state = ControllerState::State7;
                            self.shutdown = None;
                            return Ok(ShutdownDisposition::Complete);
                        }
                        ControllerState::State6 => {
                            shutdown_task7_8_transport::set_completion_policy(resources.registrations, UsbCompletionPolicy::Suppressed);
                            shutdown_task7_8_transport::unregister_primary(resources.registrations, resources.registry);
                            self.state = ControllerState::State7;
                            self.shutdown = None;
                            return Ok(ShutdownDisposition::Complete);
                        }
                        ControllerState::PendingVendor => {
                            shutdown_task7_8_transport::set_completion_policy(resources.registrations, UsbCompletionPolicy::Suppressed);
                        }
                        _ => {}
                    }
                    self.shutdown = Some(match self.state {
                        ControllerState::State8 | ControllerState::State9 | ControllerState::ActiveVendor | ControllerState::PendingVendor => ShutdownStage::Teardown,
                        _ => ShutdownStage::Cleanup,
                    });
                }
                Some(ShutdownStage::Teardown) => {
                    match resources.teardown.resume().map_err(ControllerError::Transport)? {
                        TeardownProgress::Pending => return Ok(ShutdownDisposition::Pending),
                        TeardownProgress::Complete => self.shutdown = Some(ShutdownStage::Cleanup),
                    }
                }
                Some(ShutdownStage::Cleanup) => {
                    if matches!(self.state, ControllerState::ActiveVendor | ControllerState::PendingVendor) {
                        if resources.registrations.secondary_mode() == UsbSecondaryMode::Mode2 {
                            crate::transport::cancel_transfer_timer(resources.short_timer, resources.registry);
                            crate::transport::cancel_transfer_timer(resources.long_timer, resources.registry);
                            resources.transfer.reset_flags();
                        }
                        shutdown_task7_8_transport::unregister_secondary(resources.registrations, resources.registry);
                        shutdown_task7_8_transport::unregister_primary(resources.registrations, resources.registry);
                    }
                    let publication = match self.state {
                        ControllerState::State2 => Some(ControllerPublication::Completion { handle: self.completion_handle }),
                        ControllerState::State8 => Some(ControllerPublication::Request { handle: self.request_handle }),
                        _ => None,
                    };
                    self.state = ControllerState::Stopped;
                    self.shutdown = Some(match publication { Some(publication) => ShutdownStage::Publish(publication), None => ShutdownStage::Acknowledge });
                }
                Some(ShutdownStage::Publish(publication)) => {
                    publish_controller_status(publication, resources.rtos).map_err(ControllerError::Acknowledgement)?;
                    self.shutdown = Some(ShutdownStage::Acknowledge);
                }
                Some(ShutdownStage::Acknowledge) => {
                    // The task-7 acknowledgement is the installed close consumer at 0x00ecfe20.
                    shutdown_task7_8::usb_release_acknowledged(resources.rtos).map_err(ControllerError::Acknowledgement)?;
                    self.shutdown = None;
                    return Ok(ShutdownDisposition::Complete);
                }
                None => return Ok(ShutdownDisposition::Ignored),
            }
        }
    }
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
