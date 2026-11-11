use super::Io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalAddress(pub u32);

#[derive(Clone, Copy)]
pub struct InterruptBinding {
    table: PhysicalAddress,
    handler: PhysicalAddress,
    default_handler: PhysicalAddress,
    controller: PhysicalAddress,
}
impl InterruptBinding {
    /// # Safety
    /// `table` contains 128 writable 24-byte IRQ records. `handler` enters the owned
    /// UART dispatcher; `default_handler` is the platform's unregistered vector.
    /// `controller` is this core's bank, 0xfffb3000 or 0xfffb4000.
    pub unsafe fn new(table: PhysicalAddress, handler: PhysicalAddress, default_handler: PhysicalAddress, controller: PhysicalAddress) -> Option<Self> {
        if table.0 & 3 != 0 || table.0.checked_add(128 * 24).is_none()
            || handler.0 == 0 || handler == default_handler
            || !matches!(controller.0, 0xfffb_3000 | 0xfffb_4000) { return None; }
        Some(Self { table, handler, default_handler, controller })
    }
    pub(super) fn enable<I: Io>(self, io: &I) {
        // Logical interrupt 90 maps to physical line107, priority13, mode1.
        let line = 107;
        let entry = self.table.0 + line * 24;
        for (offset, value) in [(0, self.handler.0), (4, 0), (8, 13), (12, 0), (16, 1)] {
            io.write32(PhysicalAddress(entry + offset), value);
        }
        let base = self.controller.0;
        let group = 4 * (line / 32);
        let bit = 1 << (line % 32);
        let pair = 4 * (line / 16);
        let field = 3 << (2 * (line % 16));
        let enabled = io.read32(PhysicalAddress(base + 0x80 + group));
        io.write32(PhysicalAddress(base + 0xa0 + group), !0);
        for offset in [0x60 + group, 0x100 + group] { modify(io, base + offset, bit, 0); }
        modify(io, base + 0x180 + pair, field, 1 << (2 * (line % 16)));
        modify(io, base + 0x140 + pair, field, 0);
        io.write32(PhysicalAddress(base + 0x400 + line * 4), self.handler.0);
        io.write32(PhysicalAddress(base + 0x800 + line * 4), 13);
        io.write32(PhysicalAddress(base + 0x120 + group), bit);
        io.write32(PhysicalAddress(base + 0x80 + group), enabled | bit);
    }
    pub(super) fn disable<I: Io>(self, io: &I) {
        let line = 107;
        let base = self.controller.0;
        let group = 4 * (line / 32);
        let bit = 1 << (line % 32);
        let pair = 4 * (line / 16);
        let field = 3 << (2 * (line % 16));
        let enabled = io.read32(PhysicalAddress(base + 0x80 + group));
        io.write32(PhysicalAddress(base + 0xa0 + group), !0);
        modify(io, base + 0x60 + group, bit, 0);
        io.write32(PhysicalAddress(base + 0xe0 + group), bit);
        modify(io, base + 0x100 + group, bit, 0);
        io.write32(PhysicalAddress(base + 0x120 + group), bit);
        modify(io, base + 0x180 + pair, field, 0);
        modify(io, base + 0x140 + pair, field, 0);
        io.write32(PhysicalAddress(base + 0x400 + line * 4), self.default_handler.0);
        io.write32(PhysicalAddress(base + 0x80 + group), enabled & !bit);
        let entry = self.table.0 + line * 24;
        for (offset, value) in [(0, self.default_handler.0), (4, 0), (8, 15), (12, 0), (16, 1)] {
            io.write32(PhysicalAddress(entry + offset), value);
        }
    }
}
fn modify<I: Io>(io: &I, address: u32, clear: u32, set: u32) {
    let address = PhysicalAddress(address);
    io.write32(address, (io.read32(address) & !clear) | set);
}
