//! Loader-B `kernel_publish_interfaces` (`0xd0430`): the root table at `0x400ddaa0`.
//! Loader A's is `primary_initializer`.

use super::layout::GuestAddress;
use super::Machine;

pub const ROOT: GuestAddress = GuestAddress(0x400d_daa0);
const DESCRIPTOR_POOL: u32 = 0x400e_be6c;
const EXTENDED_POOL: u32 = 0x400e_fecc;
const TICK: u32 = 0x400c_0800;
const STACK_TOP: u32 = 0x400f_3420;
pub const DESCRIPTORS: u32 = 120;
const DESCRIPTOR_TABLE: u32 = 0x178;
const DESCRIPTOR_SIZE: u32 = 44;

/// Root slots `+0x000..+0x170`, from the stock loader-B stores.
pub const SERVICES: [u32; 93] = [
    0xdd054, 0xdd168, 0xd50b0, 0xd4eec, 0xd5230, 0xd53b0, 0xd54b0, 0xd57ec,
    0xd5b00, 0xd5c94, 0xd5e14, 0xd5fc4, 0xd61c0, 0xd6358, 0xd64f4, 0xd6774,
    0xd69c0, 0xd6c40, 0xd6df8, 0xd6e08, 0xd6f8c, 0xd7170, 0xd7314, 0xd73e8,
    0xd7500, 0xdd26c, 0xdd2fc, 0xd75c4, 0xd7804, 0xd7a14, 0xd7c54, 0xd7d90,
    0xdd360, 0xdd3f8, 0xd80fc, 0xd835c, 0xd8580, 0xd86b8, 0xd896c, 0xd8af0,
    0xdd460, 0xdd504, 0xd8fb8, 0xd92c8, 0xd94e8, 0xd96d8, 0xd9aac, 0xd9cec,
    0xd9eec, 0xda2b0, 0xda52c, 0xdd580, 0xdd618, 0xdaa04, 0xdad08, 0xdaf5c,
    0xdb0a4, 0xdd688, 0xdd754, 0xdb440, 0xdb690, 0xdb7d4, 0xdba78, 0xdbd0c,
    0xdbd84, 0xd4d4c, 0xdd7dc, 0xdd86c, 0xdbf8c, 0xdc0fc, 0xdc3f4, 0xdc24c,
    0xdc4f0, 0xdc32c, 0xdc5e0, 0xdc668, 0xdc86c, 0xdc8b8, 0xdc904, 0xdc950,
    0xdc99c, 0xdca80, 0xdcb98, 0xdcbc0, 0xdcbdc, 0xdcc04, 0xd1108, 0xd1124,
    0xdd8c0, 0xd1140, 0xd1164, 0xd118c, 0xd11cc,
];

/// `+0x1618..+0x1624`.
const ADAPTERS: [u32; 4] = [0xd12bc, 0xd12cc, 0xd12e8, 0xd1304];

#[inline(never)]
pub fn publish<M: Machine>(machine: &M) {
    for (index, service) in SERVICES.iter().enumerate() {
        machine.store(ROOT.offset(index as u32 * 4), *service);
    }
    machine.store(ROOT.offset(0x174), TICK);
    for (index, adapter) in ADAPTERS.iter().enumerate() {
        machine.store(ROOT.offset(0x1618 + index as u32 * 4), *adapter);
    }
    machine.store(ROOT.offset(0x1628), STACK_TOP);
    for index in 0..DESCRIPTORS {
        let record = ROOT.offset(DESCRIPTOR_TABLE + index * DESCRIPTOR_SIZE);
        machine.store(record, index + 1);
        for field in 0..8 {
            machine.store(record.offset(4 + field * 4), DESCRIPTOR_POOL + index * 40 + field * 4);
        }
        let extended = EXTENDED_POOL + index * 56 + 24;
        machine.store(record.offset(36), extended);
        machine.store(record.offset(40), extended + 4);
    }
}
