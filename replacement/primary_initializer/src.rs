//! Primary loader-A interface publication for X-M1 1.50.
#![no_std]

use core::ptr::write_volatile;

#[derive(Clone, Copy)]
struct GuestAddress(u32);

#[derive(Clone, Copy)]
struct ByteOffset(u32);

#[derive(Clone, Copy)]
struct DescriptorIndex(u32);

#[derive(Clone, Copy)]
struct WordIndex(u32);

const INTERFACE: GuestAddress = GuestAddress(0x4004_dae0);
const DESCRIPTOR_POOL: GuestAddress = GuestAddress(0x4005_beac);
const EXTENDED_POOL: GuestAddress = GuestAddress(0x4005_ff0c);
const TICK: GuestAddress = GuestAddress(0x400c_0800);
const STACK_TOP: GuestAddress = GuestAddress(0x4006_3460);

// A-specific service entries from the canonical loader-A publication.
const SERVICES: [GuestAddress; 93] = [
    GuestAddress(0x4d094), GuestAddress(0x4d1a4), GuestAddress(0x450f0), GuestAddress(0x44f2c),
    GuestAddress(0x45270), GuestAddress(0x453f0), GuestAddress(0x454f0), GuestAddress(0x4582c),
    GuestAddress(0x45b40), GuestAddress(0x45cd4), GuestAddress(0x45e54), GuestAddress(0x46004),
    GuestAddress(0x46200), GuestAddress(0x46398), GuestAddress(0x46534), GuestAddress(0x467b4),
    GuestAddress(0x46a00), GuestAddress(0x46c80), GuestAddress(0x46e38), GuestAddress(0x46e48),
    GuestAddress(0x46fcc), GuestAddress(0x471b0), GuestAddress(0x47354), GuestAddress(0x47428),
    GuestAddress(0x47540), GuestAddress(0x4d2a8), GuestAddress(0x4d338), GuestAddress(0x47604),
    GuestAddress(0x47844), GuestAddress(0x47a54), GuestAddress(0x47c94), GuestAddress(0x47dd0),
    GuestAddress(0x4d39c), GuestAddress(0x4d434), GuestAddress(0x4813c), GuestAddress(0x4839c),
    GuestAddress(0x485c0), GuestAddress(0x486f8), GuestAddress(0x489ac), GuestAddress(0x48b30),
    GuestAddress(0x4d49c), GuestAddress(0x4d540), GuestAddress(0x48ff8), GuestAddress(0x49308),
    GuestAddress(0x49528), GuestAddress(0x49718), GuestAddress(0x49aec), GuestAddress(0x49d2c),
    GuestAddress(0x49f2c), GuestAddress(0x4a2f0), GuestAddress(0x4a56c), GuestAddress(0x4d5bc),
    GuestAddress(0x4d654), GuestAddress(0x4aa44), GuestAddress(0x4ad48), GuestAddress(0x4af9c),
    GuestAddress(0x4b0e4), GuestAddress(0x4d6c4), GuestAddress(0x4d790), GuestAddress(0x4b480),
    GuestAddress(0x4b6d0), GuestAddress(0x4b814), GuestAddress(0x4bab8), GuestAddress(0x4bd4c),
    GuestAddress(0x4bdc4), GuestAddress(0x44d8c), GuestAddress(0x4d818), GuestAddress(0x4d8a8),
    GuestAddress(0x4bfcc), GuestAddress(0x4c13c), GuestAddress(0x4c434), GuestAddress(0x4c28c),
    GuestAddress(0x4c530), GuestAddress(0x4c36c), GuestAddress(0x4c620), GuestAddress(0x4c6a8),
    GuestAddress(0x4c8ac), GuestAddress(0x4c8f8), GuestAddress(0x4c944), GuestAddress(0x4c990),
    GuestAddress(0x4c9dc), GuestAddress(0x4cac0), GuestAddress(0x4cbd8), GuestAddress(0x4cc00),
    GuestAddress(0x4cc1c), GuestAddress(0x4cc44), GuestAddress(0x41108), GuestAddress(0x41124),
    GuestAddress(0x4d8fc), GuestAddress(0x41140), GuestAddress(0x41164), GuestAddress(0x4118c),
    GuestAddress(0x411cc),
];

impl GuestAddress {
    #[inline(always)]
    fn offset(self, offset: ByteOffset) -> Self {
        Self(self.0 + offset.0)
    }
}

impl WordIndex {
    #[inline(always)]
    fn byte_offset(self) -> ByteOffset {
        ByteOffset(self.0 * 4)
    }
}

#[inline(always)]
unsafe fn publish(offset: ByteOffset, value: u32) {
    write_volatile(INTERFACE.offset(offset).0 as *mut u32, value);
}

impl DescriptorIndex {
    #[inline(always)]
    unsafe fn publish(self) {
        let record = ByteOffset(0x178 + self.0 * 44);
        publish(record, self.0 + 1);
        let descriptor = DESCRIPTOR_POOL.offset(ByteOffset(self.0 * 40));
        for field in 0..8 {
            let field = WordIndex(field).byte_offset();
            publish(ByteOffset(record.0 + 4 + field.0), descriptor.offset(field).0);
        }
        let extended = EXTENDED_POOL.offset(ByteOffset(self.0 * 56 + 24));
        publish(ByteOffset(record.0 + 36), extended.0);
        publish(ByteOffset(record.0 + 40), extended.offset(ByteOffset(4)).0);
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_primary_initializer_publish() {
    for (index, service) in SERVICES.iter().enumerate() {
        publish(WordIndex(index as u32).byte_offset(), service.0);
    }
    publish(ByteOffset(0x174), TICK.0);
    publish(ByteOffset(0x1618), GuestAddress(0x412bc).0);
    publish(ByteOffset(0x161c), GuestAddress(0x412cc).0);
    publish(ByteOffset(0x1620), GuestAddress(0x412e8).0);
    publish(ByteOffset(0x1624), GuestAddress(0x41304).0);
    publish(ByteOffset(0x1628), STACK_TOP.0);
    for index in 0..120 {
        DescriptorIndex(index).publish();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
