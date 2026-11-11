//! Vendor radio command boundary at X-M1 1.50 0x00dbd458.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3,
//! app/connectivity/ptp/service_network_commands.c, exact definition.

use super::vendor_wireless_radio_program::{
    RadioChannelRequest, RadioMode, RadioParameterProgram, RadioRate, RadioSelection,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceCommandError {
    Selection,
    Mode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelByte(u8);

impl ChannelByte {
    fn channel(self) -> RadioChannelRequest {
        RadioChannelRequest::from_abi(i32::from(self.0))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioTestConfiguration {
    Configured {
        selection: RadioSelection,
        channel: ChannelByte,
        rate: RadioRate,
        extended_rate: bool,
    },
    DirectReceive,
    DirectTransmit { channel: ChannelByte },
}

impl RadioTestConfiguration {
    pub fn program(self) -> RadioParameterProgram {
        let (mode, selection, channel, rate, extended) = match self {
            Self::Configured { selection, channel, rate, extended_rate } => {
                (RadioMode::Configured, selection, channel.channel(), rate, extended_rate)
            }
            Self::DirectReceive => {
                (RadioMode::Direct, RadioSelection::Receive, RadioChannelRequest::from_abi(0), RadioRate(0), false)
            }
            Self::DirectTransmit { channel } => {
                (RadioMode::Direct, RadioSelection::Transmit, channel.channel(), RadioRate(0), false)
            }
        };
        RadioParameterProgram::with_channel_request(mode, selection, channel, rate, extended)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioTestOrdering {
    ConfigureThenStopLan,
    ShutdownThenConfigure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioTestCommand {
    configuration: RadioTestConfiguration,
    ordering: RadioTestOrdering,
}

impl RadioTestCommand {
    pub const fn configuration(self) -> RadioTestConfiguration { self.configuration }

    pub const fn ordering(self) -> RadioTestOrdering { self.ordering }

    /// Bytes are the four sampled configuration entries, in key order.
    pub fn from_configuration_entries(entries: [u8; 4]) -> Result<Self, ServiceCommandError> {
        let mode = match entries[3] {
            0 => RadioMode::Configured,
            1 => RadioMode::Direct,
            _ => return Err(ServiceCommandError::Mode),
        };
        let selection = match entries[0] {
            0 => RadioSelection::Receive,
            1 => RadioSelection::Transmit,
            _ => return Err(ServiceCommandError::Selection),
        };
        let ordering = match selection {
            RadioSelection::Receive => RadioTestOrdering::ConfigureThenStopLan,
            RadioSelection::Transmit => RadioTestOrdering::ShutdownThenConfigure,
        };
        let configuration = match mode {
            RadioMode::Configured => {
                let extended_rate = entries[2] & 0x80 != 0;
                let rate = match selection {
                    RadioSelection::Receive => i32::from(entries[2]),
                    RadioSelection::Transmit => {
                        let rate = i32::from(entries[2] & 0x7f);
                        if rate > 11 { (rate - 12) | i32::MIN } else { rate }
                    }
                };
                RadioTestConfiguration::Configured {
                    selection,
                    channel: ChannelByte(entries[1]),
                    rate: RadioRate(rate),
                    extended_rate,
                }
            }
            RadioMode::Direct => match selection {
                RadioSelection::Receive => RadioTestConfiguration::DirectReceive,
                RadioSelection::Transmit => RadioTestConfiguration::DirectTransmit { channel: ChannelByte(entries[1]) },
            },
        };
        Ok(Self { configuration, ordering })
    }
}

#[cfg(test)]
#[path = "vendor_wireless_service_tests.rs"]
mod tests;
