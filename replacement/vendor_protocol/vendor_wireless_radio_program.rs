//! X-M1 radio test parameter packing at 0x012ad460 and 0x012997d0.
//!
//! Both are grounded in authenticated overlay47 A32 instructions at x70view
//! 55436fc85c0560dd252b0410af6f2427fe198cd3. This module constructs the
//! parameter requests and driver command buffer. It does not execute the
//! service mailbox, SDIO transmit queue, or radio-mode lifecycle.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioProgramError {
    InvalidChannel,
    InvalidMode,
    InvalidSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioChannel(i32);

impl RadioChannel {
    pub const fn from_abi(channel: i32) -> Result<Self, RadioProgramError> {
        if channel > 14 { Err(RadioProgramError::InvalidChannel) } else { Ok(Self(channel)) }
    }

    pub const fn frequency_khz(self) -> i32 {
        if self.0 == 14 { 2_484_000 } else { self.0.wrapping_sub(1).wrapping_mul(5000).wrapping_add(2_412_000) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioChannelRequest { Valid(RadioChannel), Rejected }

impl RadioChannelRequest {
    pub const fn from_abi(channel: i32) -> Self {
        match RadioChannel::from_abi(channel) {
            Ok(channel) => Self::Valid(channel),
            Err(_) => Self::Rejected,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioRate(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioSelection {
    Receive,
    Transmit,
}

impl RadioSelection {
    pub const fn from_abi(value: i32) -> Result<Self, RadioProgramError> {
        match value {
            0 => Ok(Self::Receive),
            1 => Ok(Self::Transmit),
            _ => Err(RadioProgramError::InvalidSelection),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioMode {
    Configured,
    Direct,
}

impl RadioMode {
    pub const fn from_abi(value: i32) -> Result<Self, RadioProgramError> {
        match value {
            0 => Ok(Self::Configured),
            2 => Ok(Self::Direct),
            _ => Err(RadioProgramError::InvalidMode),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ParameterSelector {
    Operation = 1,
    Rate = 3,
    SignedRateMode = 4,
    Filter = 7,
    ExtendedRate = 15,
    Frequency = 18,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioParameter {
    pub selector: ParameterSelector,
    value: i32,
}

impl RadioParameter {
    const fn new(selector: ParameterSelector, value: i32) -> Self { Self { selector, value } }

    pub const fn value(self) -> i32 { self.value }

    pub fn encode(self) -> [u8; 8] {
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&(self.selector as u32).to_le_bytes());
        bytes[4..].copy_from_slice(&self.value.to_le_bytes());
        bytes
    }

    pub fn write_driver_command(self, sequence: CommandSequence, command: &mut [u8; 20]) {
        command[..2].copy_from_slice(&20u16.to_le_bytes());
        command[4] = 1;
        command[5] = 1;
        command[6] = sequence.0;
        command[8] = 1;
        command[12..16].copy_from_slice(&(self.selector as u32).to_le_bytes());
        command[16..20].copy_from_slice(&self.value.to_le_bytes());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSequence(u8);

impl CommandSequence {
    pub const fn from_last_issued(value: u8) -> Self { Self(value) }

    pub const fn wire(self) -> u8 { self.0 }

    pub fn issue_next(&mut self) -> Self {
        self.0 = self.0.wrapping_add(1);
        *self
    }
}

pub struct RadioParameterProgram {
    mode: RadioMode,
    selection: RadioSelection,
    channel: RadioChannelRequest,
    rate: RadioRate,
    extended_rate: bool,
    next: u8,
}

impl RadioParameterProgram {
    pub const fn new(mode: RadioMode, selection: RadioSelection, channel: RadioChannel, rate: RadioRate, extended_rate: bool) -> Self {
        Self::with_channel_request(mode, selection, RadioChannelRequest::Valid(channel), rate, extended_rate)
    }

    pub const fn with_channel_request(mode: RadioMode, selection: RadioSelection,
        channel: RadioChannelRequest, rate: RadioRate, extended_rate: bool) -> Self {
        Self { mode, selection, channel, rate, extended_rate, next: 0 }
    }

    pub const fn terminal_error(&self) -> Option<RadioProgramError> {
        if matches!(self.channel, RadioChannelRequest::Rejected)
            && !matches!((self.mode, self.selection), (RadioMode::Direct, RadioSelection::Receive)) {
            Some(RadioProgramError::InvalidChannel)
        } else { None }
    }

    pub const fn remaining(&self) -> u8 {
        if self.terminal_error().is_some() {
            let total = if matches!(self.mode, RadioMode::Configured) { 1 } else { 0 };
            return total - self.next;
        }
        let total = match (self.mode, self.selection) {
            (RadioMode::Direct, RadioSelection::Receive) => 1,
            (RadioMode::Direct, RadioSelection::Transmit) => 2,
            (RadioMode::Configured, RadioSelection::Receive) => 3,
            (RadioMode::Configured, RadioSelection::Transmit) => 7,
        };
        total - self.next
    }
}

impl Iterator for RadioParameterProgram {
    type Item = RadioParameter;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining() == 0 { return None; }
        let frequency = match self.channel {
            RadioChannelRequest::Valid(channel) => channel.frequency_khz(),
            RadioChannelRequest::Rejected => 0,
        };
        use ParameterSelector::{ExtendedRate, Filter, Frequency, Operation, Rate, SignedRateMode};
        let request = match (self.mode, self.selection, self.next) {
            (RadioMode::Direct, RadioSelection::Receive, 0) => RadioParameter::new(Operation, 0),
            (RadioMode::Direct, RadioSelection::Transmit, 0) => RadioParameter::new(Frequency, frequency),
            (RadioMode::Direct, RadioSelection::Transmit, 1) => RadioParameter::new(Operation, 5),
            (RadioMode::Configured, _, 0) => RadioParameter::new(Operation, 0),
            (RadioMode::Configured, _, 1) => RadioParameter::new(Frequency, frequency),
            (RadioMode::Configured, RadioSelection::Receive, 2) => RadioParameter::new(Operation, 2),
            (RadioMode::Configured, RadioSelection::Transmit, 2) => RadioParameter::new(Filter, 0),
            (RadioMode::Configured, RadioSelection::Transmit, 3) => RadioParameter::new(SignedRateMode, if self.rate.0 < 0 { 2 } else { 0 }),
            (RadioMode::Configured, RadioSelection::Transmit, 4) => RadioParameter::new(ExtendedRate, i32::from(self.rate.0 < 0 && self.extended_rate)),
            (RadioMode::Configured, RadioSelection::Transmit, 5) => RadioParameter::new(Rate, self.rate.0),
            (RadioMode::Configured, RadioSelection::Transmit, 6) => RadioParameter::new(Operation, 1),
            _ => return None,
        };
        self.next += 1;
        Some(request)
    }
}

#[cfg(test)]
#[path = "vendor_wireless_radio_program_tests.rs"]
mod tests;
