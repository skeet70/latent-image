//! Boot queries at 0x01295968 and 0x01295b6c, confirmed against X-M1 1.50 A32 bytes.

use super::vendor_wireless_network::MacAddress;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Query { Address, Capabilities, PublishCapabilities }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError { Truncated, WrongPacketType, WrongEvent, Rejected(CommandStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandStatus { Failure, Unsupported, Other(u8) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareVersion(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareCapabilities {
    pub product: u16,
    pub version: FirmwareVersion,
    pub build: u16,
    pub feature: u8,
    pub hardware_band: u8,
    pub eeprom_used: bool,
    pub preserve_invalid_address: bool,
    pub compilation: u32,
    pub features: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryReply { Address(MacAddress), Capabilities(FirmwareCapabilities), Acknowledged }

impl Query {
    pub const fn length(self) -> usize { match self { Self::Address | Self::PublishCapabilities => 20, Self::Capabilities => 28 } }
    pub fn encode(self, sequence: u8, bytes: &mut [u8; 28]) -> usize {
        bytes.fill(0);
        let length = self.length();
        bytes[..2].copy_from_slice(&(length as u16).to_le_bytes());
        bytes[3] = 0x50;
        bytes[4] = match self { Self::Address | Self::PublishCapabilities => 0xc1, Self::Capabilities => 0x80 };
        bytes[6] = sequence;
        if self == Self::PublishCapabilities {
            bytes[5] = 1;
            bytes[8..14].fill(0xff);
            bytes[16] = 7;
            bytes[18] = 7;
        }
        length
    }
    pub fn parse(self, bytes: &[u8]) -> Result<QueryReply, QueryError> {
        let minimum = match self { Self::Address => 14, Self::Capabilities => 28, Self::PublishCapabilities => 10 };
        if bytes.len() < minimum { return Err(QueryError::Truncated); }
        if u16::from_le_bytes([bytes[2], bytes[3]]) != 1 { return Err(QueryError::WrongPacketType); }
        let event = match self { Self::Address => 9, Self::Capabilities => 2, Self::PublishCapabilities => 1 };
        if bytes[4] != event { return Err(QueryError::WrongEvent); }
        Ok(match self {
            Self::Address => QueryReply::Address(MacAddress(bytes[8..14].try_into().unwrap())),
            Self::PublishCapabilities => {
                if bytes[9] != 0 {
                    return Err(QueryError::Rejected(match bytes[9] { 1 => CommandStatus::Failure, 2 => CommandStatus::Unsupported, other => CommandStatus::Other(other) }));
                }
                QueryReply::Acknowledged
            }
            Self::Capabilities => QueryReply::Capabilities(FirmwareCapabilities {
                product: u16::from_le_bytes([bytes[8], bytes[9]]),
                version: FirmwareVersion(u16::from_le_bytes([bytes[10], bytes[11]])),
                build: u16::from_le_bytes([bytes[12], bytes[13]]),
                feature: bytes[14], hardware_band: bytes[15], eeprom_used: bytes[16] != 0,
                preserve_invalid_address: bytes[17] == 1,
                compilation: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
                features: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            }),
        })
    }
}

#[cfg(test)]
#[path = "vendor_wireless_boot_query_tests.rs"]
mod tests;
