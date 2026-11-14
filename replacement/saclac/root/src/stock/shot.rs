use xm1_shot_sequence::state::{StateAddress, StateMemory};
use xm1_saclac_shared::stock::Machine;

/// The same machine-backed memory used by the record and calculation adapters.
pub struct ShotMemory<M>(pub M);

impl<M: Machine> StateMemory for ShotMemory<M> {
    fn read(&mut self, at: StateAddress, into: &mut [u8]) {
        let mut address = at.get();
        let mut into = into;
        while !into.is_empty() {
            let width = if address & 3 == 0 && into.len() >= 4 { 4 } else if address & 1 == 0 && into.len() >= 2 { 2 } else { 1 };
            let value = match width {
                4 => self.0.read_u32(address),
                2 => u32::from(self.0.read_u16(address)),
                _ => u32::from((self.0.read_u16(address & !1) >> (8 * (address & 1))) as u8),
            };
            into[..width].copy_from_slice(&value.to_le_bytes()[..width]);
            into = &mut into[width..];
            address += width as u32;
        }
    }

    fn write(&mut self, at: StateAddress, bytes: &[u8]) {
        let mut address = at.get();
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let width = if address & 3 == 0 && bytes.len() >= 4 { 4 } else if address & 1 == 0 && bytes.len() >= 2 { 2 } else { 1 };
            match width {
                4 => self.0.write_u32(address, u32::from_le_bytes(bytes[..4].try_into().expect("state word"))),
                2 => self.0.write_u16(address, u16::from_le_bytes(bytes[..2].try_into().expect("state halfword"))),
                _ => self.0.write_u8(address, bytes[0]),
            }
            bytes = &bytes[width..];
            address += width as u32;
        }
    }

    fn copy(&mut self, to: StateAddress, from: StateAddress, bytes: usize) {
        for offset in 0..bytes as u32 {
            let source = from.get() + offset;
            let byte = (self.0.read_u16(source & !1) >> (8 * (source & 1))) as u8;
            self.0.write_u8(to.get() + offset, byte);
        }
    }
}
