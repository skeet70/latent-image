//! X-M1 1.50 interface operation state and remote properties.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::discovery::{NetworkDiscoveryRuntime, NetworkPacketLease, NetworkPacketPool, PacketPoolError};
use crate::filesystem_catalog::Catalog;
use crate::filesystem_runtime::SectorIo;

#[path = "vendor_wireless_service_task_interface_modes_catalog.rs"]
pub mod catalog;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceMode { Mode0, Mode1, Mode2, Mode3, Mode5 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceSlot { PrepareControl, ReleaseControl, Initialize, Stop, Configure, CancelConfigure, Reset, CancelReset, Start }
impl InterfaceMode {
    pub const fn supports(self, slot: InterfaceSlot) -> bool {
        !matches!((self, slot), (Self::Mode0, InterfaceSlot::Reset | InterfaceSlot::CancelReset) | (Self::Mode1 | Self::Mode2 | Self::Mode5, InterfaceSlot::Start))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceStatus { Success, Failed, InvalidConfiguration, InvalidTransfer, ProtocolFailure, Source(InterfaceSourceStatus) }
impl InterfaceStatus {
    pub const fn raw(self) -> i32 { match self { Self::Success => 0, Self::Failed => -255, Self::InvalidConfiguration => 0x80000000u32 as i32, Self::InvalidTransfer => 0x80000800u32 as i32, Self::ProtocolFailure => -51, Self::Source(status) => status.0 } }
    pub const fn from_raw(status: i32) -> Self { match status { 0 => Self::Success, -255 => Self::Failed, -51 => Self::ProtocolFailure, value => Self::Source(InterfaceSourceStatus(value)) } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceSourceStatus(i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceError { CompletionCapacity, InvalidHeader, InvalidName, InvalidTimestamp, TransferBounds, GenerationExhausted, Packet(PacketPoolError), Catalog(catalog::CatalogFailure) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceCompletionKind { Initialized, Configured, Reset, Started }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceCompletion { pub kind: InterfaceCompletionKind, pub status: InterfaceStatus }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemotePropertyCode { Mode, Configure, ObjectCount, Reset }
impl RemotePropertyCode {
    pub const fn wire(self) -> u16 { match self { Self::Mode => 0xdf00, Self::Configure => 0xd220, Self::ObjectCount => 0xd222, Self::Reset => 0xd52f } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteProperty { pub code: RemotePropertyCode, pub value: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyList { entries: [Option<RemoteProperty>; 8], length: usize }
impl PropertyList {
    pub const fn new() -> Self { Self { entries: [None; 8], length: 0 } }
    pub fn entries(&self) -> impl Iterator<Item = RemoteProperty> + '_ { self.entries[..self.length].iter().filter_map(|entry| *entry) }
    fn push(&mut self, property: RemoteProperty) { if self.length < self.entries.len() { self.entries[self.length] = Some(property); self.length += 1; } }
}
impl Default for PropertyList { fn default() -> Self { Self::new() } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteMode { Transfer, Control, Playback }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferWindow { pub offset: u32, pub length: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteControl {
    properties: PropertyList,
    mode: RemoteMode,
    interface: InterfaceMode,
    selected: u16,
    configure: bool,
    reset: bool,
    object_count: u32,
    window: TransferWindow,
}
impl RemoteControl {
    pub const fn new() -> Self { Self { properties: PropertyList::new(), mode: RemoteMode::Transfer, interface: InterfaceMode::Mode0, selected: 0, configure: false, reset: false, object_count: 0, window: TransferWindow { offset: 0, length: 0 } } }
    pub fn initialize(&mut self, mode: InterfaceMode) {
        *self = Self::new(); self.interface = match mode { InterfaceMode::Mode2 => InterfaceMode::Mode1, other => other };
        self.mode = match mode { InterfaceMode::Mode1 | InterfaceMode::Mode2 => RemoteMode::Control, _ => RemoteMode::Transfer };
    }
    pub const fn mode(&self) -> RemoteMode { self.mode }
    pub const fn interface(&self) -> InterfaceMode { self.interface }
    pub const fn selected(&self) -> u16 { self.selected }
    pub const fn configuring(&self) -> bool { self.configure }
    pub const fn resetting(&self) -> bool { self.reset }
    pub const fn object_count(&self) -> u32 { self.object_count }
    pub const fn window(&self) -> TransferWindow { self.window }
    pub fn bind_window(&mut self, window: TransferWindow) { self.window = window; }
    pub fn select(&mut self, selection: RemoteSelection) {
        self.selected = selection.wire();
        self.properties.push(RemoteProperty { code: RemotePropertyCode::Mode, value: u32::from(self.selected) });
    }
    pub fn begin_configure(&mut self) { self.configure = true; self.properties.push(RemoteProperty { code: RemotePropertyCode::Configure, value: 1 }); }
    pub fn cancel_configure(&mut self) { self.configure = false; }
    pub fn stop(&mut self) { self.selected = 0; }
    pub fn publish_count(&mut self, count: u32) { self.object_count = count; self.properties.push(RemoteProperty { code: RemotePropertyCode::ObjectCount, value: count }); }
    pub fn begin_reset(&mut self) { self.reset = true; self.properties.push(RemoteProperty { code: RemotePropertyCode::Reset, value: 1 }); }
    pub fn finish_reset(&mut self) { self.reset = false; }
    pub fn take_properties(&mut self) -> PropertyList { core::mem::take(&mut self.properties) }
}
impl Default for RemoteControl { fn default() -> Self { Self::new() } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteSelection { Transfer, Configure, Reset, Playback }
impl RemoteSelection { pub const fn wire(self) -> u16 { match self { Self::Transfer => 3, Self::Configure => 1, Self::Reset => 17, Self::Playback => 2 } } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFormat { Jpeg, Movie, Raw, Audio }
impl ObjectFormat {
    pub const fn transfer_code(self) -> u16 { match self { Self::Jpeg => 0x3808, Self::Movie => 0xb901, Self::Raw => 0xb902, Self::Audio => 0x3808 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectName { bytes: [u8; 48], length: usize }
impl ObjectName {
    pub fn parse(bytes: &[u8]) -> Result<Self, InterfaceError> {
        if bytes.len() >= 48 || bytes.contains(&0) { return Err(InterfaceError::InvalidName); }
        let mut name = Self { bytes: [0; 48], length: bytes.len() };
        name.bytes[..bytes.len()].copy_from_slice(bytes); Ok(name)
    }
    pub fn from_header(header: &[u8]) -> Result<Self, InterfaceError> {
        if header.first() != Some(&0x5a) { return Err(InterfaceError::InvalidHeader); }
        let name = header.get(17..).ok_or(InterfaceError::InvalidHeader)?;
        let length = name.iter().position(|byte| *byte == 0).ok_or(InterfaceError::InvalidName)?;
        Self::parse(&name[..length])
    }
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectTimestamp([u8; 16]);
impl ObjectTimestamp {
    pub fn parse(bytes: &[u8]) -> Result<Self, InterfaceError> {
        if bytes.len() != 17 { return Err(InterfaceError::InvalidTimestamp); }
        let mut result = [0; 16]; result[..2].copy_from_slice(b"20");
        for (target, source) in [0,1,3,4,6,7,9,10,12,13,15,16].into_iter().enumerate() { result[target + 2] = bytes[source]; }
        Ok(Self(result))
    }
    pub const fn bytes(&self) -> &[u8; 16] { &self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferLength(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferOffset { Primary(u32), Alternate }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceDataRequest { pub offset: TransferOffset, pub maximum: Option<TransferLength> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryObject { Record0, Record1 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationGeneration(u64);
pub struct InterfaceConfiguration {
    pub name: ObjectName,
    pub timestamp: ObjectTimestamp,
    pub format: ObjectFormat,
    pub payload: NetworkPacketLease,
    pub offset: u32,
    pub alternate: NetworkPacketLease,
}
impl InterfaceConfiguration {
    pub fn release(self, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> { InterfaceModes::release_configuration(self, discovery) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMetadata { pub name: ObjectName, pub timestamp: ObjectTimestamp, pub format: ObjectFormat, pub length: u32 }

pub struct InterfaceModes {
    metadata: [Option<ObjectMetadata>; 3],
    configurations: [Option<InterfaceConfiguration>; 3],
    cancelled: [bool; 3],
    data_requests: [Option<InterfaceDataRequest>; 3],
    data_ready: [bool; 3],
    generations: [ConfigurationGeneration; 3],
    completions: [Option<InterfaceCompletion>; 8],
    read: usize,
    length: usize,
}
impl Default for InterfaceModes { fn default() -> Self { Self::new() } }
impl InterfaceModes {
    pub const fn new() -> Self { Self { metadata: [None; 3], configurations: [None, None, None], cancelled: [false; 3], data_requests: [None; 3], data_ready: [false; 3], generations: [ConfigurationGeneration(0); 3], completions: [None; 8], read: 0, length: 0 } }
    fn index(mode: InterfaceMode) -> usize { match mode { InterfaceMode::Mode0 => 0, InterfaceMode::Mode1 | InterfaceMode::Mode2 => 1, InterfaceMode::Mode3 | InterfaceMode::Mode5 => 2 } }
    pub fn metadata(&self, mode: InterfaceMode) -> Option<ObjectMetadata> { if mode == InterfaceMode::Mode5 { None } else { self.metadata[Self::index(mode)] } }
    pub const fn configuration_generation(&self, mode: InterfaceMode) -> ConfigurationGeneration { self.generations[match mode { InterfaceMode::Mode0 => 0, InterfaceMode::Mode1 | InterfaceMode::Mode2 => 1, InterfaceMode::Mode3 | InterfaceMode::Mode5 => 2 }] }
    pub fn partial_reads_unsupported(&self, mode: InterfaceMode) -> bool { mode != InterfaceMode::Mode5 && self.cancelled[Self::index(mode)] }
    pub fn bind_data_request(&mut self, mode: InterfaceMode, request: InterfaceDataRequest) {
        let index = Self::index(mode);
        self.data_requests[index] = Some(request);
        self.data_ready[index] = false;
    }
    pub fn clear_data_request(&mut self, mode: InterfaceMode) {
        let index = Self::index(mode);
        self.data_requests[index] = None;
        self.data_ready[index] = false;
    }
    pub fn take_data_ready(&mut self) -> Option<(InterfaceMode, InterfaceDataRequest)> {
        for (index, mode) in [InterfaceMode::Mode0, InterfaceMode::Mode1, InterfaceMode::Mode3].into_iter().enumerate() {
            if self.data_ready[index] {
                self.data_ready[index] = false;
                if let Some(request) = self.data_requests[index] { return Some((mode, request)); }
            }
        }
        None
    }
    pub fn completion(&mut self) -> Option<InterfaceCompletion> { if self.length == 0 { return None; } let result = self.completions[self.read].take(); self.read = (self.read + 1) % self.completions.len(); self.length -= 1; result }
    fn notify(&mut self, kind: InterfaceCompletionKind, status: InterfaceStatus) -> Result<(), InterfaceError> { if self.length == self.completions.len() { return Err(InterfaceError::CompletionCapacity); } let index = (self.read + self.length) % self.completions.len(); self.completions[index] = Some(InterfaceCompletion { kind, status }); self.length += 1; Ok(()) }
    pub fn receive_protocol_completion(&mut self, kind: InterfaceCompletionKind, status: InterfaceStatus) -> Result<(), InterfaceError> {
        self.notify(kind, status)
    }
    pub fn initialize<I: SectorIo>(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>, filesystem: &mut Catalog<'_, I>) -> Result<(), InterfaceError> {
        if self.length == self.completions.len() { return Err(InterfaceError::CompletionCapacity); }
        if mode == InterfaceMode::Mode5 { return self.notify(InterfaceCompletionKind::Initialized, InterfaceStatus::Success); }
        self.teardown(mode, discovery)?;
        discovery.interface_control.initialize(mode);
        let status = match mode {
            InterfaceMode::Mode0 => discovery.catalog.initialize_primary(filesystem).map_err(|error| error.status()),
            InterfaceMode::Mode3 => discovery.catalog.initialize_mode3(filesystem).map(|_| ()).map_err(|error| error.status()),
            InterfaceMode::Mode1 | InterfaceMode::Mode2 | InterfaceMode::Mode5 => Ok(()),
        };
        self.notify(InterfaceCompletionKind::Initialized, InterfaceStatus::from_raw(status.err().unwrap_or(0)))
    }
    pub fn start<I: SectorIo>(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>, filesystem: &mut Catalog<'_, I>) -> Result<(), InterfaceError> {
        if !matches!(mode, InterfaceMode::Mode0 | InterfaceMode::Mode3) { return Ok(()); }
        if mode == InterfaceMode::Mode3 {
            discovery.interface_control.select(RemoteSelection::Playback);
            let publication = discovery.catalog.start_mode3().map_err(InterfaceError::Catalog)?;
            discovery.interface_control.publish_count(publication.count);
            return Ok(());
        }
        if self.length == self.completions.len() { return Err(InterfaceError::CompletionCapacity); }
        discovery.interface_control.select(RemoteSelection::Transfer);
        let result = discovery.catalog.scan_primary(filesystem);
        let status = match result {
            Ok(publication) => { discovery.interface_control.publish_count(publication.count); InterfaceStatus::Success }
            Err(error) => InterfaceStatus::from_raw(error.status()),
        };
        self.notify(InterfaceCompletionKind::Started, status)
    }
    pub fn prepare_control(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode != InterfaceMode::Mode5 { discovery.prepare_control(); }
        Ok(())
    }
    pub fn release_control(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode != InterfaceMode::Mode5 { discovery.release_control(); }
        Ok(())
    }
    pub fn stop(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode != InterfaceMode::Mode5 { discovery.interface_control.stop(); }
        Ok(())
    }
    pub fn configure(&mut self, mode: InterfaceMode, configuration: InterfaceConfiguration, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode == InterfaceMode::Mode5 {
            Self::release_configuration(configuration, discovery)?;
            return self.notify(InterfaceCompletionKind::Configured, InterfaceStatus::Failed);
        }
        let index = Self::index(mode);
        let length = match discovery.packets.len(&configuration.payload).map_err(InterfaceError::Packet).and_then(|length| u32::try_from(length).map_err(|_| InterfaceError::TransferBounds)) {
            Ok(length) => length,
            Err(error) => { Self::release_configuration(configuration, discovery)?; return Err(error); }
        };
        if let Err(error) = discovery.packets.len(&configuration.alternate) {
            Self::release_configuration(configuration, discovery)?;
            return Err(InterfaceError::Packet(error));
        }
        if self.cancelled[index] {
            self.metadata[index] = None;
            discovery.interface_control.cancel_configure();
            self.cancelled[index] = false;
        }
        if matches!(mode, InterfaceMode::Mode1 | InterfaceMode::Mode2) && self.metadata[index].is_some() {
            return Self::release_configuration(configuration, discovery);
        }
        let generation = match self.generations[index].0.checked_add(1) {
            Some(generation) => ConfigurationGeneration(generation),
            None => { Self::release_configuration(configuration, discovery)?; return Err(InterfaceError::GenerationExhausted); }
        };
        self.generations[index] = generation;
        if let Some(previous) = self.configurations[index].take() {
            if let Err(error) = Self::release_configuration(previous, discovery) {
                Self::release_configuration(configuration, discovery)?;
                self.metadata[index] = None;
                return Err(error);
            }
        }
        self.metadata[index] = Some(ObjectMetadata { name: configuration.name, timestamp: configuration.timestamp, format: configuration.format, length });
        discovery.interface_control.bind_window(TransferWindow { offset: configuration.offset, length });
        self.configurations[index] = Some(configuration);
        if matches!(mode, InterfaceMode::Mode0 | InterfaceMode::Mode3) && self.data_requests[index].is_some() { self.data_ready[index] = true; }
        if matches!(mode, InterfaceMode::Mode1 | InterfaceMode::Mode2) {
            discovery.interface_control.select(RemoteSelection::Configure);
            discovery.interface_control.begin_configure();
        }
        Ok(())
    }
    fn release_configuration(configuration: InterfaceConfiguration, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        let primary = discovery.packets.release(configuration.payload);
        let alternate = discovery.packets.release(configuration.alternate);
        primary.and(alternate).map_err(InterfaceError::Packet)
    }
    pub fn teardown(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode == InterfaceMode::Mode5 { return Ok(()); }
        let index = Self::index(mode);
        self.generations[index] = ConfigurationGeneration(self.generations[index].0.checked_add(1).ok_or(InterfaceError::GenerationExhausted)?);
        self.metadata[index] = None;
        self.cancelled[index] = false;
        self.clear_data_request(mode);
        if let Some(configuration) = self.configurations[index].take() { Self::release_configuration(configuration, discovery)?; }
        Ok(())
    }
    pub fn delete_memory_object(&mut self, mode: InterfaceMode, object: MemoryObject, packets: &mut NetworkPacketPool<'_>, control: &mut RemoteControl) -> Result<InterfaceStatus, InterfaceError> {
        control.cancel_configure();
        let index = Self::index(mode);
        self.cancelled[index] = false;
        if !matches!(mode, InterfaceMode::Mode1 | InterfaceMode::Mode2) || object != MemoryObject::Record1 || self.metadata(mode).is_none() {
            return Ok(InterfaceStatus::from_raw(0x80000100u32 as i32));
        }
        self.generations[index] = ConfigurationGeneration(self.generations[index].0.checked_add(1).ok_or(InterfaceError::GenerationExhausted)?);
        self.metadata[index] = None;
        self.clear_data_request(mode);
        if let Some(configuration) = self.configurations[index].take() {
            let primary = packets.release(configuration.payload);
            let alternate = packets.release(configuration.alternate);
            primary.and(alternate).map_err(InterfaceError::Packet)?;
        }
        Ok(InterfaceStatus::Success)
    }
    pub fn transfer_data<'a>(&'a self, mode: InterfaceMode, offset: TransferOffset, maximum: Option<TransferLength>, packets: &'a NetworkPacketPool<'_>) -> Result<&'a [u8], InterfaceError> {
        let configuration = self.configurations[Self::index(mode)].as_ref().ok_or(InterfaceError::TransferBounds)?;
        let bytes = match offset {
            TransferOffset::Alternate => packets.bytes(&configuration.alternate).map_err(InterfaceError::Packet)?,
            TransferOffset::Primary(offset) => {
                let relative = if matches!(mode, InterfaceMode::Mode1 | InterfaceMode::Mode2) { offset } else { offset.checked_sub(configuration.offset).ok_or(InterfaceError::TransferBounds)? };
                packets.bytes(&configuration.payload).map_err(InterfaceError::Packet)?.get(relative as usize..).ok_or(InterfaceError::TransferBounds)?
            }
        };
        Ok(match maximum { Some(TransferLength(limit)) if limit != 0 && (limit as usize) < bytes.len() => &bytes[..limit as usize], _ => bytes })
    }
    pub fn cancel_configure(&mut self, mode: InterfaceMode, _discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if mode != InterfaceMode::Mode5 {
            let index = Self::index(mode);
            if self.metadata[index].is_some() { self.cancelled[index] = true; }
        }
        Ok(())
    }
    pub fn reset(&mut self, mode: InterfaceMode, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> {
        if matches!(mode, InterfaceMode::Mode1 | InterfaceMode::Mode2 | InterfaceMode::Mode3) {
            discovery.interface_control.select(RemoteSelection::Reset);
            discovery.interface_control.begin_reset();
        }
        Ok(())
    }
    pub fn cancel_reset(&mut self, _mode: InterfaceMode, _discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), InterfaceError> { Ok(()) }
}

#[cfg(test)]
#[path = "vendor_wireless_service_task_interface_modes_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "vendor_wireless_service_task_interface_modes_lease_tests.rs"]
mod lease_tests;
