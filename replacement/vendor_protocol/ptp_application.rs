//! X-M1 1.50 mode-two operation identities, session rules, and property snapshots.

use super::ptp_runtime::PtpRuntime;
use super::ptp_types::{BatteryLevel, PtpCommand, PtpOperationCode, PtpPropertyCode, PtpResponse, PtpResponseCode, PtpSessionId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationOperation {
    DeviceInformation, OpenSession, CloseSession, StorageIdentifiers, StorageInformation,
    ObjectCount, ObjectHandles, ObjectInformation, Object, Thumbnail, DeleteObject,
    SendObjectInformation, SendObject, InitiateCapture, PropertyDescriptor, PropertyValue,
    SetPropertyValue, PartialObject, VendorSendObjectInformation, VendorSendObject,
    VendorObjectData, ObjectPropertiesSupported, ObjectPropertyDescriptor,
    ObjectPropertyValue, ObjectPropertyList,
}
impl ApplicationOperation {
    pub const fn decode(code: PtpOperationCode) -> Option<Self> {
        Some(match code.0 {
            0x1001 => Self::DeviceInformation, 0x1002 => Self::OpenSession,
            0x1003 => Self::CloseSession, 0x1004 => Self::StorageIdentifiers,
            0x1005 => Self::StorageInformation, 0x1006 => Self::ObjectCount,
            0x1007 => Self::ObjectHandles, 0x1008 => Self::ObjectInformation,
            0x1009 => Self::Object, 0x100a => Self::Thumbnail, 0x100b => Self::DeleteObject,
            0x100c => Self::SendObjectInformation, 0x100d => Self::SendObject,
            0x100f => Self::InitiateCapture, 0x1014 => Self::PropertyDescriptor,
            0x1015 => Self::PropertyValue, 0x1016 => Self::SetPropertyValue,
            0x101b => Self::PartialObject, 0x900c => Self::VendorSendObjectInformation,
            0x900d => Self::VendorSendObject, 0x901d => Self::VendorObjectData,
            0x9801 => Self::ObjectPropertiesSupported, 0x9802 => Self::ObjectPropertyDescriptor,
            0x9803 => Self::ObjectPropertyValue, 0x9805 => Self::ObjectPropertyList,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationError { ShortOutput { required: usize }, InvalidDataset, PropertyNotSupported }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitiatorVersion<'a>(&'a [u8]);
impl<'a> InitiatorVersion<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ApplicationError> {
        let count = *bytes.first().ok_or(ApplicationError::InvalidDataset)? as usize;
        if bytes.len() != count * 2 + 1 { return Err(ApplicationError::InvalidDataset); }
        Ok(Self(bytes))
    }
}

pub struct DeviceProperties<'a> {
    initiator: InitiatorVersion<'a>,
}
impl Default for DeviceProperties<'_> { fn default() -> Self { Self::new() } }
impl<'a> DeviceProperties<'a> {
    pub const fn new() -> Self { Self { initiator: InitiatorVersion(&[1, 0, 0]) } }
    pub const fn snapshot(initiator: InitiatorVersion<'a>) -> Self { Self { initiator } }
    pub fn value(&self, property: PtpPropertyCode, battery: BatteryLevel, output: &mut [u8]) -> Result<usize, ApplicationError> {
        match property.0 {
            0x5001 => copy_dataset(output, &[battery as u8]),
            0xd303 => copy_dataset(output, &[1]),
            0xd406 => copy_dataset(output, self.initiator.0),
            0xd407 => copy_dataset(output, &1u32.to_le_bytes()),
            _ => Err(ApplicationError::PropertyNotSupported),
        }
    }
    pub fn descriptor(&self, property: PtpPropertyCode, battery: BatteryLevel, output: &mut [u8]) -> Result<usize, ApplicationError> {
        let (datatype, writable, default): (u16, u8, &[u8]) = match property.0 {
            0x5001 => (2, 0, &[3]), 0xd303 => (2, 0, &[1]),
            0xd406 => (0xffff, 1, &[1, 0, 0]), 0xd407 => (6, 0, &[1, 0, 0, 0]),
            _ => return Err(ApplicationError::PropertyNotSupported),
        };
        let current_bytes = match property.0 { 0xd406 => self.initiator.0.len(), 0xd407 => 4, _ => 1 };
        let form: &[u8] = if property.0 == 0x5001 { &[1, 0, 3, 1] } else { &[0] };
        let bytes = 5 + default.len() + current_bytes + form.len();
        require_output(output, bytes)?;
        output[..2].copy_from_slice(&property.0.to_le_bytes());
        output[2..4].copy_from_slice(&datatype.to_le_bytes());
        output[4] = writable;
        let cursor = 5 + default.len();
        output[5..cursor].copy_from_slice(default);
        self.value(property, battery, &mut output[cursor..cursor + current_bytes])?;
        output[cursor + current_bytes..bytes].copy_from_slice(form);
        Ok(bytes)
    }
}

fn require_output(output: &[u8], required: usize) -> Result<(), ApplicationError> {
    if output.len() < required { Err(ApplicationError::ShortOutput { required }) } else { Ok(()) }
}
fn copy_dataset(output: &mut [u8], data: &[u8]) -> Result<usize, ApplicationError> {
    require_output(output, data.len())?;
    output[..data.len()].copy_from_slice(data);
    Ok(data.len())
}

/// OpenSession over the session identifier `PtpRuntime` owns.
pub fn open_session(runtime: &mut PtpRuntime, command: &PtpCommand) -> PtpResponse {
    let code = if !zero_suffix(command, 1) { PtpResponseCode(0x2006) }
        else if command.parameters[0] == 0 { PtpResponseCode::INVALID_PARAMETER }
        else if runtime.session().is_some() { PtpResponseCode::SESSION_ALREADY_OPEN }
        else { runtime.set_session(Some(PtpSessionId(command.parameters[0]))); PtpResponseCode::OK };
    response(command, code)
}
pub fn close_session(runtime: &mut PtpRuntime, command: &PtpCommand) -> PtpResponse {
    let code = if runtime.session().is_none() { PtpResponseCode::SESSION_NOT_OPEN }
        else if !zero_suffix(command, 0) { PtpResponseCode(0x2006) }
        else { runtime.set_session(None); PtpResponseCode::OK };
    response(command, code)
}

fn zero_suffix(command: &PtpCommand, used: usize) -> bool {
    command.parameters[used..].iter().all(|parameter| *parameter == 0)
}
fn response(command: &PtpCommand, code: PtpResponseCode) -> PtpResponse {
    PtpResponse { code, transaction: command.transaction, parameters: [0; 5], parameter_count: 0 }
}
