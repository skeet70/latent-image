//! X-M1 1.50 remote property values, source pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::executor::{validate_unused, OperationReply, OperationRequest, PtpError, PtpOutput, PtpPhase, PtpState, ResponseCode};
use super::packet::{OperationCode, TransactionId};
use super::super::interface_modes::RemoteControl;

#[path = "vendor_wireless_service_task_discovery_executor_properties_objects.rs"]
pub mod objects;
#[path = "vendor_wireless_service_task_discovery_executor_properties_schema.rs"]
pub mod schema;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyIdentifier(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyCode {
    Battery, FunctionalMode, FriendlyName, DeviceVersion, PerceivedType,
    Configure, ObjectCount, Reset, Configuration, Selection, SelectionAcknowledgement,
    TransferMinimum, TransferLimit, PlaybackMinimum, TransferOffset, TransferLength,
    AlternateMinimum, Destination,
}
impl PropertyCode {
    pub const fn parse(value: u16) -> Option<Self> {
        Some(match value {
            0x5001 => Self::Battery, 0xd303 => Self::FunctionalMode,
            0xd406 => Self::FriendlyName, 0xd407 => Self::DeviceVersion,
            0xd207 => Self::PerceivedType, 0xd220 => Self::Configure,
            0xd222 => Self::ObjectCount, 0xd52f => Self::Reset,
            0xd500 => Self::Configuration, 0xdf00 => Self::Selection,
            0xdf01 => Self::SelectionAcknowledgement, 0xdf21 => Self::TransferMinimum,
            0xdf22 => Self::TransferLimit, 0xdf31 => Self::PlaybackMinimum,
            0xd226 => Self::TransferOffset, 0xd227 => Self::TransferLength,
            0xdf23 => Self::AlternateMinimum, 0xd228 => Self::Destination,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyStatus { Success, ReadOnly, InvalidValue, InvalidLength }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MinimumRequest(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyOffset(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyLength(pub u16);

pub struct RemotePropertyValues {
    friendly_name: [u8; 512],
    configuration: [u8; 145],
    destination: [u8; 128],
    selection_acknowledgement: u16,
    offset: PropertyOffset,
    length: PropertyLength,
    minimum_requests: [MinimumRequest; 4],
}
impl RemotePropertyValues {
    pub const fn new() -> Self {
        Self { friendly_name: [0; 512], configuration: [0; 145], destination: [0; 128],
            selection_acknowledgement: 0, offset: PropertyOffset(0), length: PropertyLength(0),
            minimum_requests: [MinimumRequest(0); 4] }
    }
    pub const fn offset(&self) -> PropertyOffset { self.offset }
    pub const fn length(&self) -> PropertyLength { self.length }
    pub const fn selection_acknowledgement(&self) -> u16 { self.selection_acknowledgement }
    pub fn configuration(&self) -> &[u8; 145] { &self.configuration }
    pub fn destination(&self) -> &[u8; 128] { &self.destination }
    pub const fn minimum_requests(&self) -> &[MinimumRequest; 4] { &self.minimum_requests }

    pub fn read(&self, code: PropertyIdentifier, control: &RemoteControl, output: &mut [u8]) -> Result<usize, PropertyStatus> {
        let Some(property) = PropertyCode::parse(code.0) else { return Ok(0); };
        let scalar: (u32, usize) = match property {
            PropertyCode::FriendlyName => {
                let length = usize::from(self.friendly_name[0]) * 2 + 1;
                let target = output.get_mut(..length).ok_or(PropertyStatus::InvalidLength)?;
                target.copy_from_slice(&self.friendly_name[..length]);
                return Ok(length);
            }
            PropertyCode::DeviceVersion => (1, 4),
            PropertyCode::PerceivedType => return Ok(0),
            PropertyCode::Configure => (u32::from(control.configuring()), 4),
            PropertyCode::ObjectCount => (control.object_count(), 4),
            PropertyCode::Reset => (u32::from(control.resetting()), 4),
            PropertyCode::Selection => (u32::from(control.selected()), 2),
            PropertyCode::SelectionAcknowledgement => (u32::from(self.selection_acknowledgement), 2),
            PropertyCode::TransferMinimum | PropertyCode::PlaybackMinimum | PropertyCode::AlternateMinimum => (1, 4),
            PropertyCode::TransferLimit => (2, 4),
            PropertyCode::TransferOffset => (u32::from(self.offset.0), 2),
            PropertyCode::TransferLength => (u32::from(self.length.0), 2),
            PropertyCode::Configuration => return Self::read_string(&self.configuration, output),
            PropertyCode::Destination => return Self::read_string(&self.destination, output),
            PropertyCode::Battery | PropertyCode::FunctionalMode => return Ok(0),
        };
        let target = output.get_mut(..scalar.1).ok_or(PropertyStatus::InvalidLength)?;
        target.copy_from_slice(&scalar.0.to_le_bytes()[..scalar.1]);
        Ok(scalar.1)
    }

    fn read_string(source: &[u8], output: &mut [u8]) -> Result<usize, PropertyStatus> {
        let length = usize::from(source[0]) * 2 + 1;
        let target = output.get_mut(..length).ok_or(PropertyStatus::InvalidLength)?;
        target.copy_from_slice(source.get(..length).ok_or(PropertyStatus::InvalidLength)?);
        Ok(length)
    }

    pub fn write(&mut self, code: PropertyIdentifier, bytes: &[u8], control: &RemoteControl) -> PropertyStatus {
        let Some(property) = PropertyCode::parse(code.0) else { return PropertyStatus::Success; };
        match property {
            PropertyCode::FriendlyName => {
                if bytes.len() > self.friendly_name.len() { return PropertyStatus::InvalidLength; }
                self.friendly_name[..bytes.len()].copy_from_slice(bytes);
            }
            PropertyCode::Configuration => {
                if bytes.is_empty() || bytes[0] == 0 || bytes.len() > self.configuration.len() { return PropertyStatus::InvalidValue; }
                self.configuration[..bytes.len()].copy_from_slice(bytes);
            }
            PropertyCode::Destination => {
                self.destination.fill(0);
                let Some(&count) = bytes.first() else { return PropertyStatus::InvalidValue; };
                if count == 0 || bytes.len() > 128 || usize::from(count) * 2 + 1 != bytes.len() { return PropertyStatus::InvalidValue; }
                for (target, source) in self.destination.iter_mut().zip(bytes[1..].chunks_exact(2)) { *target = source[0]; }
            }
            PropertyCode::SelectionAcknowledgement => {
                let Some(value) = Self::halfword(bytes) else { return PropertyStatus::InvalidLength; };
                self.selection_acknowledgement = value;
                if value != control.selected() { return PropertyStatus::InvalidValue; }
            }
            PropertyCode::TransferMinimum | PropertyCode::TransferLimit | PropertyCode::PlaybackMinimum | PropertyCode::AlternateMinimum => {
                let Some(value) = bytes.get(..4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])) else { return PropertyStatus::InvalidLength; };
                let (index, minimum) = match property {
                    PropertyCode::TransferMinimum => (0, 1), PropertyCode::TransferLimit => (1, 2),
                    PropertyCode::AlternateMinimum => (2, 1), _ => (3, 1),
                };
                self.minimum_requests[index] = MinimumRequest(value);
                if value < minimum { return PropertyStatus::InvalidValue; }
            }
            PropertyCode::TransferOffset => {
                let Some(value) = Self::halfword(bytes) else { return PropertyStatus::InvalidLength; };
                self.offset = PropertyOffset(value);
            }
            PropertyCode::TransferLength => {
                let Some(value) = Self::halfword(bytes) else { return PropertyStatus::InvalidLength; };
                self.length = PropertyLength(value);
            }
            PropertyCode::Battery | PropertyCode::FunctionalMode | PropertyCode::PerceivedType => return PropertyStatus::Success,
            _ => return PropertyStatus::ReadOnly,
        }
        PropertyStatus::Success
    }
    fn halfword(bytes: &[u8]) -> Option<u16> { bytes.get(..2).map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])) }
}
impl Default for RemotePropertyValues { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PropertyTransaction {
    Idle,
    Descriptor { transaction: TransactionId },
    Read { transaction: TransactionId },
    Write { transaction: TransactionId, property: PropertyIdentifier },
}

pub struct RemotePropertyOperations {
    values: RemotePropertyValues,
    transaction: PropertyTransaction,
}
impl RemotePropertyOperations {
    pub const fn new() -> Self { Self { values: RemotePropertyValues::new(), transaction: PropertyTransaction::Idle } }
    pub fn values(&self) -> &RemotePropertyValues { &self.values }
    pub fn values_mut(&mut self) -> &mut RemotePropertyValues { &mut self.values }
    pub const fn pending(&self) -> bool { !matches!(self.transaction, PropertyTransaction::Idle) }
    pub fn teardown(&mut self) { self.transaction = PropertyTransaction::Idle; }

    pub fn execute(&mut self, request: OperationRequest<'_>, state: &mut PtpState, output: &mut PtpOutput, control: &mut RemoteControl) -> Result<OperationReply, PtpError> {
        if !matches!(request.command.operation.0, 0x1014..=0x1016 | 0x9801 | 0x9802) { return Err(PtpError::NotConfigured); }
        if !output.is_empty() || output.data_remaining() != 0 { return Err(PtpError::OutputFull); }
        let transaction = request.command.transaction;
        let mut reply = OperationReply::new(ResponseCode(0x2001));
        if request.phase == PtpPhase::Cancel {
            self.teardown();
            reply.response = state.response;
            output.send_response(reply, transaction)?;
            return Ok(reply);
        }
        match request.phase {
            PtpPhase::Command => {
                if self.pending() { return Err(PtpError::InvalidState); }
                if matches!(request.command.operation.0, 0x9801 | 0x9802) {
                    return self.execute_schema(request, state, output);
                }
                if state.session_id.is_none() { reply.response = ResponseCode(0x2003); }
                else if !validate_unused(request.command, 1) { reply.response = ResponseCode(0x2006); }
                if reply.response.0 != 0x2001 {
                    output.send_response(reply, transaction)?;
                } else {
                    match request.command.operation.0 {
                        0x1014 => self.transaction = PropertyTransaction::Descriptor { transaction },
                        0x1015 => {
                            if !output.is_empty() { return Err(PtpError::OutputFull); }
                            let property = request.command.parameters[0] as u16;
                            let mut bytes = [0; 512];
                            let length = if property == 0xd212 {
                                let list = control.take_properties();
                                let count = list.entries().count();
                                bytes[..2].copy_from_slice(&(count as u16).to_le_bytes());
                                for (entry, target) in list.entries().zip(bytes[2..].chunks_exact_mut(6)) {
                                    target[..2].copy_from_slice(&entry.code.wire().to_le_bytes());
                                    target[2..].copy_from_slice(&entry.value.to_le_bytes());
                                }
                                2 + count * 6
                            } else {
                                self.values.read(PropertyIdentifier(property), control, &mut bytes).map_err(|_| PtpError::InvalidLength)?
                            };
                            output.send_data(OperationCode(0x1015), transaction, &bytes[..length])?;
                            self.transaction = PropertyTransaction::Read { transaction };
                        }
                        _ => self.transaction = PropertyTransaction::Write { transaction, property: PropertyIdentifier(request.command.parameters[0] as u16) },
                    }
                }
            }
            PtpPhase::Data => {
                match self.transaction {
                    PropertyTransaction::Read { transaction: expected } if expected == transaction => {}
                    PropertyTransaction::Write { transaction: expected, property } if expected == transaction => {
                        let status = self.values.write(property, request.data, control);
                        if status != PropertyStatus::Success { reply.response = ResponseCode(0x2002); }
                    }
                    _ => return Err(PtpError::InvalidState),
                }
                output.send_response(reply, transaction)?;
                self.transaction = PropertyTransaction::Idle;
            }
            PtpPhase::Cancel => unreachable!(),
        }
        state.response = reply.response;
        Ok(reply)
    }

    fn execute_schema(&mut self, request: OperationRequest<'_>, state: &mut PtpState, output: &mut PtpOutput) -> Result<OperationReply, PtpError> {
        let mut reply = OperationReply::new(ResponseCode(0x2001));
        let command = request.command;
        let mut bytes = [0; 64];
        let mut length = 0;
        if state.session_id.is_none() {
            reply.response = ResponseCode(0x2003);
        } else if command.operation.0 == 0x9801 {
            if !validate_unused(command, 1) {
                reply.response = ResponseCode(0x2006);
            } else if let Some(format) = schema::ObjectFormatId::parse(command.parameters[0] as u16) {
                let properties = format.properties();
                bytes[..4].copy_from_slice(&(properties.len() as u32).to_le_bytes());
                for (property, target) in properties.iter().zip(bytes[4..].chunks_exact_mut(2)) { target.copy_from_slice(&property.to_le_bytes()); }
                length = 4 + properties.len() * 2;
            } else {
                reply.response = ResponseCode(0x200b);
            }
        } else if let Some(property) = schema::ObjectPropertyId::parse(command.parameters[0] as u16) {
            if schema::ObjectFormatId::parse(command.parameters[1] as u16).is_none() {
                reply.response = ResponseCode(0x200b);
            } else if !validate_unused(command, 2) {
                reply.response = ResponseCode(0x2006);
            } else {
                length = schema::ObjectPropertyDescriptor::for_property(property).encode(&mut bytes).ok_or(PtpError::InvalidLength)?;
            }
        } else {
            reply.response = ResponseCode(0xa801);
        }
        if reply.response.0 == 0x2001 {
            output.send_data(command.operation, command.transaction, &bytes[..length])?;
            self.transaction = PropertyTransaction::Read { transaction: command.transaction };
        } else {
            output.send_response(reply, command.transaction)?;
        }
        state.response = reply.response;
        Ok(reply)
    }
}
impl Default for RemotePropertyOperations { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_executor_properties_tests.rs"]
mod tests;
