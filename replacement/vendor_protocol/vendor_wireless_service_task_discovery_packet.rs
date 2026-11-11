#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationCode(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError {
    Truncated,
    LengthMismatch,
    InvalidCommandParameters,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferKind { Invalid, Command, Other }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferClassification {
    pub kind: TransferKind,
    pub status: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerHandshake {
    pub identity: [u8; 16],
    pub protocol: u32,
    pub name: [u16; 27],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CameraIdentity {
    pub name: [u16; 20],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerMac(pub [u8; 6]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerName(pub [u8; 27]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRestriction { pub name: PeerName, pub address: PeerMac }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandPacket {
    pub operation: OperationCode,
    pub transaction: TransactionId,
    pub parameters: [u32; 5],
    pub parameter_count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationPacket<'a> {
    Command(CommandPacket),
    Data { operation: OperationCode, transaction: TransactionId, bytes: &'a [u8] },
    Unsupported { kind: u16, operation: OperationCode, transaction: TransactionId },
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, PacketError> {
    let value = bytes.get(offset..offset + 4).ok_or(PacketError::Truncated)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

impl PeerHandshake {
    pub fn parse(bytes: &[u8]) -> Result<Option<Self>, PacketError> {
        if word(bytes, 0)? as usize != bytes.len() { return Err(PacketError::LengthMismatch); }
        if word(bytes, 4)? != 1 { return Ok(None); }
        let mut identity = [0; 16];
        identity.copy_from_slice(bytes.get(8..24).ok_or(PacketError::Truncated)?);
        let protocol = word(bytes, 24)?;
        let name_bytes = bytes.get(28..82).ok_or(PacketError::Truncated)?;
        let mut name = [0; 27];
        for (output, input) in name.iter_mut().zip(name_bytes.chunks_exact(2)) {
            *output = u16::from_le_bytes([input[0], input[1]]);
        }
        Ok(Some(Self { identity, protocol, name }))
    }
}

impl<'a> ApplicationPacket<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if word(bytes, 0)? as usize != bytes.len() { return Err(PacketError::LengthMismatch); }
        let header = bytes.get(..12).ok_or(PacketError::Truncated)?;
        let kind = u16::from_le_bytes([header[4], header[5]]);
        let operation = OperationCode(u16::from_le_bytes([header[6], header[7]]));
        let transaction = TransactionId(word(bytes, 8)?);
        match kind {
            1 => {
                let payload = &bytes[12..];
                if payload.len() % 4 != 0 || payload.len() > 20 { return Err(PacketError::InvalidCommandParameters); }
                let mut parameters = [0; 5];
                for (output, input) in parameters.iter_mut().zip(payload.chunks_exact(4)) {
                    *output = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
                }
                Ok(Self::Command(CommandPacket { operation, transaction, parameters, parameter_count: (payload.len() / 4) as u8 }))
            }
            2 => Ok(Self::Data { operation, transaction, bytes: &bytes[12..] }),
            _ => Ok(Self::Unsupported { kind, operation, transaction }),
        }
    }
}

pub fn response_frame(status: i32, identity: CameraIdentity) -> ([u8; 68], usize) {
    let mut frame = [0; 68];
    if status == 0 {
        frame[..4].copy_from_slice(&68u32.to_le_bytes());
        frame[4..8].copy_from_slice(&2u32.to_le_bytes());
        frame[12..28].copy_from_slice(&[0x08, 0x70, 0xb0, 0x61, 0x0a, 0x8b, 0x45, 0x93, 0xb2, 0xe7, 0x93, 0x57, 0xdd, 0x36, 0xe0, 0x50]);
        for (output, character) in frame[28..].chunks_exact_mut(2).zip(identity.name) {
            output.copy_from_slice(&character.to_le_bytes());
        }
        (frame, 68)
    } else {
        frame[..4].copy_from_slice(&12u32.to_le_bytes());
        frame[4..8].copy_from_slice(&5u32.to_le_bytes());
        frame[8..12].copy_from_slice(&status.to_le_bytes());
        (frame, 12)
    }
}

pub const fn finish_frame() -> [u8; 8] { [8, 0, 0, 0, 255, 255, 255, 255] }
