//! Core-1 interface publication after the loader-B completion wait.
#![no_std]

#[allow(unused_attributes)]
#[path = "../loader_prefix/src.rs"]
mod loader_prefix;

use core::ptr::write_volatile;

#[derive(Clone, Copy)]
struct RetainedAddress(u32);

#[derive(Clone, Copy)]
struct InterfaceOffset(u32);

#[derive(Clone, Copy)]
struct DescriptorIndex(u32);

const INTERFACE: u32 = 0x400d_daa0;
const DESCRIPTOR_POOL: RetainedAddress = RetainedAddress(0x400e_be6c);
const EXTENDED_POOL: RetainedAddress = RetainedAddress(0x400e_fecc);
const TICK: RetainedAddress = RetainedAddress(0x400c_0800);
const STACK_TOP: RetainedAddress = RetainedAddress(0x400f_3420);

const SERVICES: [RetainedAddress; 93] = [
    RetainedAddress(0xdd054), RetainedAddress(0xdd168), RetainedAddress(0xd50b0), RetainedAddress(0xd4eec),
    RetainedAddress(0xd5230), RetainedAddress(0xd53b0), RetainedAddress(0xd54b0), RetainedAddress(0xd57ec),
    RetainedAddress(0xd5b00), RetainedAddress(0xd5c94), RetainedAddress(0xd5e14), RetainedAddress(0xd5fc4),
    RetainedAddress(0xd61c0), RetainedAddress(0xd6358), RetainedAddress(0xd64f4), RetainedAddress(0xd6774),
    RetainedAddress(0xd69c0), RetainedAddress(0xd6c40), RetainedAddress(0xd6df8), RetainedAddress(0xd6e08),
    RetainedAddress(0xd6f8c), RetainedAddress(0xd7170), RetainedAddress(0xd7314), RetainedAddress(0xd73e8),
    RetainedAddress(0xd7500), RetainedAddress(0xdd26c), RetainedAddress(0xdd2fc), RetainedAddress(0xd75c4),
    RetainedAddress(0xd7804), RetainedAddress(0xd7a14), RetainedAddress(0xd7c54), RetainedAddress(0xd7d90),
    RetainedAddress(0xdd360), RetainedAddress(0xdd3f8), RetainedAddress(0xd80fc), RetainedAddress(0xd835c),
    RetainedAddress(0xd8580), RetainedAddress(0xd86b8), RetainedAddress(0xd896c), RetainedAddress(0xd8af0),
    RetainedAddress(0xdd460), RetainedAddress(0xdd504), RetainedAddress(0xd8fb8), RetainedAddress(0xd92c8),
    RetainedAddress(0xd94e8), RetainedAddress(0xd96d8), RetainedAddress(0xd9aac), RetainedAddress(0xd9cec),
    RetainedAddress(0xd9eec), RetainedAddress(0xda2b0), RetainedAddress(0xda52c), RetainedAddress(0xdd580),
    RetainedAddress(0xdd618), RetainedAddress(0xdaa04), RetainedAddress(0xdad08), RetainedAddress(0xdaf5c),
    RetainedAddress(0xdb0a4), RetainedAddress(0xdd688), RetainedAddress(0xdd754), RetainedAddress(0xdb440),
    RetainedAddress(0xdb690), RetainedAddress(0xdb7d4), RetainedAddress(0xdba78), RetainedAddress(0xdbd0c),
    RetainedAddress(0xdbd84), RetainedAddress(0xd4d4c), RetainedAddress(0xdd7dc), RetainedAddress(0xdd86c),
    RetainedAddress(0xdbf8c), RetainedAddress(0xdc0fc), RetainedAddress(0xdc3f4), RetainedAddress(0xdc24c),
    RetainedAddress(0xdc4f0), RetainedAddress(0xdc32c), RetainedAddress(0xdc5e0), RetainedAddress(0xdc668),
    RetainedAddress(0xdc86c), RetainedAddress(0xdc8b8), RetainedAddress(0xdc904), RetainedAddress(0xdc950),
    RetainedAddress(0xdc99c), RetainedAddress(0xdca80), RetainedAddress(0xdcb98), RetainedAddress(0xdcbc0),
    RetainedAddress(0xdcbdc), RetainedAddress(0xdcc04), RetainedAddress(0xd1108), RetainedAddress(0xd1124),
    RetainedAddress(0xdd8c0), RetainedAddress(0xd1140), RetainedAddress(0xd1164), RetainedAddress(0xd118c),
    RetainedAddress(0xd11cc),
];

#[inline(always)]
unsafe fn publish(offset: InterfaceOffset, value: u32) {
    write_volatile((INTERFACE + offset.0) as *mut u32, value);
}

impl DescriptorIndex {
    #[inline(always)]
    unsafe fn publish(self) {
        let record = 0x178 + self.0 * 44;
        publish(InterfaceOffset(record), self.0 + 1);
        for field in 0..8 {
            publish(InterfaceOffset(record + 4 + field * 4), DESCRIPTOR_POOL.0 + self.0 * 40 + field * 4);
        }
        let extended = EXTENDED_POOL.0 + self.0 * 56 + 24;
        publish(InterfaceOffset(record + 36), extended);
        publish(InterfaceOffset(record + 40), extended + 4);
    }
}

// Entry requires the admitted writable core-1 workspace and retained pools.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_interface_initializer_publish() {
    for (index, service) in SERVICES.iter().enumerate() {
        publish(InterfaceOffset(index as u32 * 4), service.0);
    }
    publish(InterfaceOffset(0x174), TICK.0);
    publish(InterfaceOffset(0x1618), RetainedAddress(0xd12bc).0);
    publish(InterfaceOffset(0x161c), RetainedAddress(0xd12cc).0);
    publish(InterfaceOffset(0x1620), RetainedAddress(0xd12e8).0);
    publish(InterfaceOffset(0x1624), RetainedAddress(0xd1304).0);
    publish(InterfaceOffset(0x1628), STACK_TOP.0);
    for index in 0..120 {
        DescriptorIndex(index).publish();
    }
}
