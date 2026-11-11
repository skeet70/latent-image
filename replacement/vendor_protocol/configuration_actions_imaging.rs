//! Imaging configuration actions with closed CPU consumers.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};

#[path = "configuration_imaging_strobe.rs"]
pub mod strobe;

#[path = "configuration_imaging_planes.rs"]
pub mod planes;

#[path = "configuration_imaging_profiles.rs"]
pub mod profiles;

#[path = "configuration_imaging_statistics.rs"]
pub mod statistics;

#[path = "configuration_imaging_capture_layout.rs"]
pub mod capture_layout;

#[path = "configuration_imaging_cache.rs"]
pub mod cache;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImmediateAction { Action76, Action77, Action81, Action82, Action83, Action84 }
impl ImmediateAction {
    pub const fn from_queue_action(action: u16) -> Option<Self> {
        match action { 76 => Some(Self::Action76), 77 => Some(Self::Action77), 81 => Some(Self::Action81), 82 => Some(Self::Action82), 83 => Some(Self::Action83), 84 => Some(Self::Action84), _ => None }
    }
    pub const fn execute(self) -> ActionCompletion { ActionCompletion::service_success() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketKind { Write0, Write1, Write2, Write3, Write4, Read20, Read21, Read22, Read23 }
impl PacketKind {
    fn parse(value: u8) -> Option<Self> {
        Some(match value { 0 => Self::Write0, 1 => Self::Write1, 2 => Self::Write2, 3 => Self::Write3, 4 => Self::Write4, 0x20 => Self::Read20, 0x21 => Self::Read21, 0x22 => Self::Read22, 0x23 => Self::Read23, _ => return None })
    }
    const fn wire(self) -> u8 { match self { Self::Write0 => 0, Self::Write1 => 1, Self::Write2 => 2, Self::Write3 => 3, Self::Write4 => 4, Self::Read20 => 0x20, Self::Read21 => 0x21, Self::Read22 => 0x22, Self::Read23 => 0x23 } }
    const fn length(self) -> usize { match self { Self::Write0 | Self::Read20 => 9, Self::Write1 | Self::Write3 | Self::Read21 | Self::Read23 => 2, Self::Write2 | Self::Read22 => 4, Self::Write4 => 3 } }
    const fn is_read(self) -> bool { matches!(self, Self::Read20 | Self::Read21 | Self::Read22 | Self::Read23) }
}

pub fn configuration_packet(configuration: &mut ConfigImages<'_>, transport: &mut impl strobe::Hardware) -> Result<ActionCompletion, BackendError> {
    let key = |index: usize| ConfigurationKey::from_abi(0x81 + index as u16);
    let Some(kind) = PacketKind::parse(key(0).read(configuration)?) else { return Ok(ActionCompletion::service_rejected()); };
    let mut packet = [0; 16];
    packet[0] = kind.wire();
    if kind.is_read() {
        strobe::read(transport, kind.wire(), &mut packet[..kind.length()]);
        let populated = if kind == PacketKind::Read20 { 7 } else { kind.length() - 1 };
        for index in 1..=10 { key(index).write(configuration, if index <= populated { packet[index] } else { 0 })?; }
    } else {
        for index in 1..kind.length() { packet[index] = key(index).read(configuration)?; }
        // dc210c ignores the serial transport result.
        strobe::write(transport, &packet[..kind.length()]);
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(target_arch = "arm")]
pub fn configuration_packet_native(configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    configuration_packet(configuration, &mut strobe::NativeHardware)
}

#[cfg(test)]
#[path = "configuration_imaging_tests.rs"]
mod tests;
