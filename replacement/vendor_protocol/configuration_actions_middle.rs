//! ABI identities and direct CPU paths for X-M1 configuration queue actions 35 through 74.
//!
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Action numbers are one greater than the slots in the table at 0xecdf84.
//! Immediate paths: 36, 38, 39, 59..62, 71 and 73.
//! Original image hashes: id4 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16,
//! id5 3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a.

use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiddleAction(u8);

impl MiddleAction {
    pub fn parse(queue_action: u8) -> Result<Self, BackendError> {
        if (35..=74).contains(&queue_action) { Ok(Self(queue_action)) }
        else { Err(BackendError::InvalidCommand) }
    }

    pub const fn queue_action(self) -> u8 { self.0 }
    pub const fn wrapper(self) -> u32 { 0xecc6fc + (self.0 as u32 - 35) * 24 }
    pub const fn target(self) -> u32 { TARGETS[self.0 as usize - 35] }
}

const TARGETS: [u32; 40] = [
    0xdbcd4c, 0xdbcf68, 0xdbcf80, 0xdbd0d4, 0xdbd3ec,
    0xdbd404, 0xdbd458, 0xdbd5e8, 0xdbd7b8, 0xdbd8bc,
    0xdbdc7c, 0xdbdfbc, 0xdbe0d0, 0xdbe3b0, 0xdbe50c,
    0xdbe568, 0xdbe5c4, 0xdbe614, 0xdbe664, 0xdbe6b4,
    0xdbe938, 0xdbea68, 0xdbeb08, 0xdbec08, 0xdbec78,
    0xdbec90, 0xdbeca8, 0xdbecc0, 0xdbecd8, 0xdbed8c,
    0xdbeedc, 0xdbef88, 0xdbf1bc, 0xdbf288, 0xdbf4dc,
    0xdbf598, 0xdbf63c, 0xdbf6a8, 0xdbf6f8, 0xdbf764,
];

#[derive(Clone, Copy)]
pub struct MiddleArguments<'a> { bytes: &'a [u8] }

impl<'a> MiddleArguments<'a> {
    pub const fn from_abi(bytes: &'a [u8]) -> Self { Self { bytes } }

    pub fn key(self, argument: usize) -> Result<ConfigurationKey, BackendError> {
        let offset = argument.checked_mul(2).ok_or(BackendError::Range)?;
        let end = offset.checked_add(2).ok_or(BackendError::Range)?;
        let bytes = self.bytes.get(offset..end).ok_or(BackendError::Capacity)?;
        Ok(ConfigurationKey::from_abi(u16::from_le_bytes([bytes[0], bytes[1]])))
    }

    pub fn read(self, argument: usize, configuration: &ConfigImages<'_>) -> Result<u8, BackendError> {
        self.key(argument)?.read(configuration)
    }

    pub fn write(self, argument: usize, configuration: &mut ConfigImages<'_>, value: u8) -> Result<(), BackendError> {
        self.key(argument)?.write(configuration, value)
    }

    pub fn read_pair(self, high_argument: usize, configuration: &ConfigImages<'_>) -> Result<u16, BackendError> {
        Ok(u16::from_be_bytes([self.read(high_argument, configuration)?, self.read(high_argument + 1, configuration)?]))
    }
}

// These targets invoke dba464(0) without reading the argument block.
pub fn execute_empty(action: MiddleAction) -> Option<ActionCompletion> {
    match action.queue_action() {
        36 | 39 | 59..=62 => Some(ActionCompletion::service_success()),
        _ => None,
    }
}

// dbd0d4 selects drive 0x41; 71f6ac admits only 0x42 and 0x43.
pub fn execute_drive_presence(
    arguments: MiddleArguments<'_>,
    configuration: &mut ConfigImages<'_>,
) -> Result<ActionCompletion, BackendError> {
    arguments.write(0, configuration, 0)?;
    Ok(ActionCompletion::service_success())
}

pub fn execute_immediate(
    action: MiddleAction,
    arguments: MiddleArguments<'_>,
    configuration: &mut ConfigImages<'_>,
) -> Option<Result<ActionCompletion, BackendError>> {
    match action.queue_action() {
        38 => Some(execute_drive_presence(arguments, configuration)),
        71 | 73 => Some(execute_record_parameter(arguments, configuration)),
        _ => execute_empty(action).map(Ok),
    }
}

// efc998 and efc9f8 synchronously invoke their non-null completion with zero.
pub fn execute_record_parameter(
    arguments: MiddleArguments<'_>,
    configuration: &mut ConfigImages<'_>,
) -> Result<ActionCompletion, BackendError> {
    let _value = arguments.read(0, configuration)?;
    arguments.write(1, configuration, 0)?;
    Ok(ActionCompletion::service_success())
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackDisposition { Accepted, Failed }

pub fn publish_byte_pair(
    disposition: CallbackDisposition,
    value: u16,
    high: ConfigurationKey,
    low: ConfigurationKey,
    configuration: &mut ConfigImages<'_>,
) -> Result<(), BackendError> {
    if disposition == CallbackDisposition::Accepted {
        let [high_value, low_value] = value.to_be_bytes();
        high.write(configuration, high_value)?;
        low.write(configuration, low_value)?;
    }
    Ok(())
}


#[path = "configuration_actions_middle_property.rs"]
pub mod property;

#[cfg(test)]
#[path = "configuration_actions_middle_tests.rs"]
mod tests;
