use super::{InterfaceOperation, TaskMessage, TaskQueue};
use super::ap::{ModeError, ModeRuntime};
use super::discovery::NetworkDiscoveryRuntime;
use super::interface_modes::{InterfaceConfiguration, InterfaceError, InterfaceMode, InterfaceModes, InterfaceCompletion};
use super::station::{StationError, StationServiceRuntime};
use super::super::vendor_wireless_network::dhcp::Milliseconds;
use super::super::vendor_wireless_network::lan::LanRuntime;
use super::super::vendor_wireless_network::transport::socket_task::{SocketTaskRuntime, SocketTaskSocketCompletion};
use super::super::vendor_wireless_network::transport::TransportTick;
use super::super::wireless_association::station::StationRuntime;
use crate::filesystem_catalog::Catalog;
use crate::filesystem_runtime::SectorIo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceSelection { SocketSlots, Station }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestedMode { Mode(InterfaceMode), Unmapped(u32) }
impl RequestedMode {
    pub const fn parse(value: u32) -> Self {
        match value { 0 => Self::Mode(InterfaceMode::Mode0), 1 => Self::Mode(InterfaceMode::Mode1), 2 => Self::Mode(InterfaceMode::Mode2), 3 => Self::Mode(InterfaceMode::Mode3), 5 => Self::Mode(InterfaceMode::Mode5), value => Self::Unmapped(value) }
    }
}
#[derive(Debug)]
pub enum ControllerError { NoInterface, Interface(InterfaceError), Slots(StationError), Station(ModeError) }
pub struct TaskContext<'a, 'storage, I: SectorIo> {
    pub socket: &'a mut SocketTaskRuntime,
    pub station: &'a mut StationRuntime,
    pub discovery: &'a mut NetworkDiscoveryRuntime<'storage>,
    pub lan: &'a mut LanRuntime<'storage>,
    pub catalog: &'a mut Catalog<'storage, I>,
    pub now: Milliseconds,
}
pub struct InterfaceRuntime {
    service: ServiceSelection,
    requested: RequestedMode,
    selected: Option<InterfaceMode>,
    pub modes: InterfaceModes,
    pub slots: StationServiceRuntime,
    pub station: ModeRuntime,
}
impl Default for InterfaceRuntime { fn default() -> Self { Self::new() } }
impl InterfaceRuntime {
    pub fn new() -> Self {
        Self { service: ServiceSelection::SocketSlots, requested: RequestedMode::Mode(InterfaceMode::Mode0), selected: None,
            modes: InterfaceModes::new(), slots: StationServiceRuntime::new(), station: ModeRuntime::new() }
    }
    pub fn set_selection(&mut self, service: ServiceSelection, requested: RequestedMode) { self.service = service; self.requested = requested; }
    pub const fn selection(&self) -> ServiceSelection { self.service }
    pub const fn selected_mode(&self) -> Option<InterfaceMode> { self.selected }
    fn select(&mut self) {
        if let RequestedMode::Mode(mode) = self.requested { self.selected = Some(mode); }
    }
    pub fn reset_service_state<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        self.slots.reset(context.discovery).map_err(ControllerError::Slots)?;
        self.station.reset();
        Ok(())
    }
    pub fn initialize<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        self.select();
        self.modes.initialize(self.selected.ok_or(ControllerError::NoInterface)?, context.discovery, context.catalog).map_err(ControllerError::Interface)
    }
    pub fn stop<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        self.modes.stop(self.selected.ok_or(ControllerError::NoInterface)?, context.discovery).map_err(ControllerError::Interface)
    }
    pub fn reset<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        self.modes.reset(self.selected.ok_or(ControllerError::NoInterface)?, context.discovery).map_err(ControllerError::Interface)
    }
    pub fn begin_operation<I: SectorIo>(&mut self, operation: InterfaceOperation, configuration: &mut Option<InterfaceConfiguration>, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        let mode = self.selected.ok_or(ControllerError::NoInterface)?;
        match operation {
            InterfaceOperation::Configure | InterfaceOperation::ConfigureNamed | InterfaceOperation::ConfigureMedia => self.modes.configure(mode, configuration.take().ok_or(ControllerError::NoInterface)?, context.discovery),
            InterfaceOperation::Reset => self.modes.reset(mode, context.discovery),
            InterfaceOperation::Start => self.modes.start(mode, context.discovery, context.catalog),
        }.map_err(ControllerError::Interface)
    }
    pub fn abort_operation<I: SectorIo>(&mut self, operation: InterfaceOperation, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        let mode = self.selected.ok_or(ControllerError::NoInterface)?;
        match operation {
            InterfaceOperation::Configure | InterfaceOperation::ConfigureNamed | InterfaceOperation::ConfigureMedia => self.modes.cancel_configure(mode, context.discovery),
            InterfaceOperation::Reset => self.modes.cancel_reset(mode, context.discovery),
            InterfaceOperation::Start => Ok(()),
        }.map_err(ControllerError::Interface)
    }
    pub fn prepare_control<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>) -> Result<(), ControllerError> {
        match self.requested {
            RequestedMode::Mode(InterfaceMode::Mode0 | InterfaceMode::Mode1 | InterfaceMode::Mode2 | InterfaceMode::Mode3) => {
                let mode = self.selected.ok_or(ControllerError::NoInterface)?;
                self.modes.prepare_control(mode, context.discovery).map_err(ControllerError::Interface)?;
                self.modes.release_control(mode, context.discovery).map_err(ControllerError::Interface)
            }
            _ => Ok(()),
        }
    }
    pub fn dispatch<I: SectorIo>(&mut self, message: &mut TaskMessage, context: &mut TaskContext<'_, '_, I>) -> Result<bool, ControllerError> {
        match self.service {
            ServiceSelection::SocketSlots => self.slots.process(message, context.socket, context.discovery).map_err(ControllerError::Slots),
            ServiceSelection::Station => self.station.dispatch(message, context.station, &mut self.slots.sockets, context.socket, context.discovery, context.lan, &mut self.modes, self.selected.ok_or(ControllerError::NoInterface)?, context.catalog, context.now).map_err(ControllerError::Station),
        }
    }
    pub fn receive_socket_completion<I: SectorIo>(&mut self, completion: SocketTaskSocketCompletion, context: &mut TaskContext<'_, '_, I>) -> Result<bool, ControllerError> {
        match self.service {
            ServiceSelection::SocketSlots => self.slots.receive_socket_completion(completion, context.socket, context.discovery).map_err(ControllerError::Slots),
            ServiceSelection::Station => self.station.receive_socket_completion(completion, &mut self.slots.sockets, context.socket, context.discovery, context.lan, &mut self.modes, self.selected.ok_or(ControllerError::NoInterface)?, context.catalog, context.now).map_err(ControllerError::Station),
        }
    }
    pub fn receive_interface_completion<I: SectorIo>(&mut self, completion: InterfaceCompletion, context: &mut TaskContext<'_, '_, I>) -> Result<bool, ControllerError> {
        match self.service {
            ServiceSelection::SocketSlots => Ok(false),
            ServiceSelection::Station => self.station.receive_interface_completion(completion, &mut self.slots.sockets, context.socket).map_err(ControllerError::Station),
        }
    }
    pub fn poll<I: SectorIo>(&mut self, context: &mut TaskContext<'_, '_, I>, queue: &mut TaskQueue<'_>) -> Result<bool, ControllerError> {
        let socket = self.slots.poll(TransportTick(context.now.0), queue, context.discovery).map_err(ControllerError::Slots)?;
        let station = self.station.poll_station(context.station, context.now).map_err(ControllerError::Station)?;
        Ok(socket || station)
    }
}
