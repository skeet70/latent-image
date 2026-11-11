use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredPart { Notification, Header, Payload, Current }

pub trait Task3Diagnostics {
    fn missing_stored_request(&mut self, part: StoredPart);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum StoredCommand {
    Command0200 = 0x200, Command0201 = 0x201, Command0202 = 0x202,
    Command0203 = 0x203, Command0204 = 0x204, Command0205 = 0x205,
    Command0206 = 0x206, Command0207 = 0x207, Command0208 = 0x208,
    Command0209 = 0x209, Command020b = 0x20b, Command020c = 0x20c,
    Command020d = 0x20d, Command020e = 0x20e, Command020f = 0x20f,
    Command0210 = 0x210, Command0212 = 0x212, Command0213 = 0x213,
    Command0214 = 0x214, Command0215 = 0x215,
}

impl StoredCommand {
    pub const fn parse(value: u32) -> Option<Self> {
        Some(match value {
            0x200 => Self::Command0200, 0x201 => Self::Command0201, 0x202 => Self::Command0202,
            0x203 => Self::Command0203, 0x204 => Self::Command0204, 0x205 => Self::Command0205,
            0x206 => Self::Command0206, 0x207 => Self::Command0207, 0x208 => Self::Command0208,
            0x209 => Self::Command0209, 0x20b => Self::Command020b, 0x20c => Self::Command020c,
            0x20d => Self::Command020d, 0x20e => Self::Command020e, 0x20f => Self::Command020f,
            0x210 => Self::Command0210, 0x212 => Self::Command0212, 0x213 => Self::Command0213,
            0x214 => Self::Command0214, 0x215 => Self::Command0215, _ => return None,
        })
    }
}

pub fn helper_dce40c<M: GuestAddressSpace, E: Task3Diagnostics>(context: &mut RecordContext<M, E>, command: StoredCommand, selection: u32, latest: bool) -> Result<u32, MemoryError> {
    use StoredCommand::*;
    let offset = match command {
        Command0200 => 0x54, Command0201 => 0x8c, Command0202 => 0xb8,
        Command0203 => 0x2fc8, Command0204 => 0x130, Command0205 => 0x15c,
        Command0206 => match selection {
            0x99 => 0x2ddc,
            0x7f | 0x87 | 0xfe | 0x101 => {
                if latest {
                    let count = context.read_u32(GuestAddress::new(0x0152_9fac))?;
                    0x1ac_u32.wrapping_add(count.wrapping_sub(1).wrapping_mul(0x38))
                } else { 0x1ac }
            }
            0x40 => 0x2e14,
            0x9a | 0x9c => 0x2da4,
            _ => { context.effects.missing_stored_request(StoredPart::Notification); return Ok(0); }
        },
        Command0207 => 0x2e4c,
        Command0208 => match selection {
            0x40 => 0x2eb4, 0x87 => 0x2e7c,
            _ => { context.effects.missing_stored_request(StoredPart::Notification); return Ok(0); }
        },
        Command0209 => 0x2eec, Command020b => 0x2f58, Command020c => 0xf0,
        Command020d => 0x2f88, Command020e => 0x3008, Command020f => 0x2f18,
        Command0210 => 0x3034, Command0212 => 0x30a4, Command0213 => 0x30dc,
        Command0214 => 0x3064, Command0215 => 0x31b8,
    };
    context.read_u32(GuestAddress::new(0x0152_6a78_u32.wrapping_add(offset)))
}
