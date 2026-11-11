//! Reply layouts from image 4 at 0x006b868c, pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::packets::ControlOperation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyHalfword(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyFlags16(pub [bool; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyData {
    StatusOnly,
    ThreeFlags([bool; 3]),
    Capabilities { flags: [bool; 13], mode: u8 },
    Halfword(ReplyHalfword),
    FlaggedUnsigned { flag: bool, value: ReplyHalfword },
    FlaggedSigned { flag: bool, value: i16 },
    FlaggedBytes { flag: bool, upper: u8, lower: u8 },
    ByteFlag { byte: u8, flag: bool },
    Flags(ReplyFlags16),
    ObserverFlags([bool; 9]),
    Channel { pages: u16, format: u8, present: bool },
}
pub fn decode(operation: ControlOperation, response: u32) -> Option<ReplyData> {
    let upper = response >> 16;
    let bit = |index: u32| response & (1 << index) != 0;
    Some(match operation.request_id().0 {
        0x721b | 0x721c => return None,
        0x7209 | 0x7212 => ReplyData::ThreeFlags([bit(16), bit(17), bit(18)]),
        0x720c => ReplyData::Capabilities { flags: [bit(31), bit(30), bit(29), bit(28), bit(26), bit(23), bit(22), bit(21), bit(20), bit(19), bit(18), bit(17), bit(16)], mode: ((response >> 24) & 3) as u8 },
        0x720d | 0x7213 | 0x7216 | 0x7239 | 0x723b..=0x723e | 0x7256 => ReplyData::Halfword(ReplyHalfword(upper as u16)),
        0x7214 => ReplyData::Halfword(ReplyHalfword((upper & 0xfff) as u16)),
        0x720e => ReplyData::FlaggedUnsigned { flag: bit(31), value: ReplyHalfword((upper & 0x3fff) as u16) },
        0x723a => ReplyData::FlaggedUnsigned { flag: bit(31), value: ReplyHalfword((upper & 0x7fff) as u16) },
        0x720f => ReplyData::FlaggedSigned { flag: bit(31), value: ((upper as i16) << 1) >> 1 },
        0x7210 => ReplyData::FlaggedBytes { flag: bit(31), upper: ((response >> 24) & 0x7f) as u8, lower: upper as u8 },
        0x7211 => ReplyData::ByteFlag { byte: upper as u8, flag: bit(31) },
        0x7215 | 0x7238 => {
            let mut flags = [false; 16];
            for (index, value) in flags.iter_mut().enumerate() { *value = bit(index as u32 + 16); }
            ReplyData::Flags(ReplyFlags16(flags))
        }
        0x7217 => ReplyData::ObserverFlags([bit(16), bit(17), bit(18), bit(19), bit(20), bit(21), bit(22), bit(24), bit(28)]),
        0x722e => ReplyData::Channel { pages: (upper & 0x7ff) as u16, format: ((response >> 27) & 15) as u8, present: bit(31) },
        _ => ReplyData::StatusOnly,
    })
}
