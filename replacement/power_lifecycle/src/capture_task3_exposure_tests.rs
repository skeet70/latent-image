use super::*;
use crate::record_address_space::{ByteCount, GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

#[derive(Default)]
struct Display { text: Option<[u8; 40]> }
impl<M: GuestAddressSpace> ExposureDisplay<M> for Display {
    fn draw_exposure_text(&mut self, memory: &mut M, text: GuestAddress, _: u8, _: u8, _: u8, _: u8) -> Result<(), RecordError> {
        let mut bytes = [0; 40];
        for (offset, value) in bytes.iter_mut().enumerate() { *value = memory.read_u8(text.checked_add(offset as u32)?)?; }
        self.text = Some(bytes);
        Ok(())
    }
}

fn region(base: u32, bytes: &mut [u8]) -> MemoryRegion<'_> {
    let descriptor = RegionDescriptor::new(GuestAddress::new(base), ByteCount(bytes.len() as u32), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    MemoryRegion::new(descriptor, bytes).unwrap()
}

fn with_context(test: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Display>)) {
    let mut recording = [0; 0x3548];
    let mut profile = [0; 2];
    let mut handle = [0; 8];
    let mut work = [0; 0x200];
    let mut settings = [0; 0x200];
    let mut resource = [0; 12];
    let mut configuration_pointer = [0; 4];
    let mut diagnostic_flag = [0; 1];
    let mut stack = [0x5a; 0x100];
    let mut regions = [
        region(0x01526a78, &mut recording), region(0x0091682e, &mut profile),
        region(0x1000, &mut handle), region(0x2000, &mut work), region(0x3000, &mut settings),
        region(0x015b5ca8, &mut resource), region(0x0096ca50, &mut configuration_pointer), region(0x1a890, &mut diagnostic_flag),
        region(0x4000, &mut stack),
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let context = &mut RecordContext::with_stack(memory, Display::default(), GuestStack::new(GuestAddress::new(0x4000), ByteCount(0x100)).unwrap());
    for (address, value) in [(0x1529fb4, 0x3000), (0x1529fb8, 0x2000), (0x1529fbc, 0x1000), (0x96ca50, 0x4000)] {
        context.write_u32(GuestAddress::new(address), value).unwrap();
    }
    test(context);
}

#[test]
fn changed_selection_updates_full_cache_and_narrowed_public_fields() {
    with_context(|context| {
        assert_eq!(helper_dcc01c(context, ExposureSelection(0x1234abcd)), Ok(true));
        assert_eq!(context.read_u32(GuestAddress::new(0x15b5ca8)), Ok(0x1234abcd));
        for address in [0x91682e, 0x20ea, 0x3124] { assert_eq!(context.read_u16(GuestAddress::new(address)), Ok(0xabcd)); }
    });
}

#[test]
fn unchanged_full_selection_preserves_independent_mirrors() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x15b5ca8), 9).unwrap();
        for (address, value) in [(0x91682e, 1), (0x20ea, 2), (0x3124, 3)] { context.write_u16(GuestAddress::new(address), value).unwrap(); }
        assert_eq!(helper_dcc01c(context, ExposureSelection(9)), Ok(false));
        for (address, value) in [(0x91682e, 1), (0x20ea, 2), (0x3124, 3)] { assert_eq!(context.read_u16(GuestAddress::new(address)), Ok(value)); }
    });
}

#[test]
fn diagnostics_preserve_overflow_residue_and_run_for_unchanged_selection() {
    with_context(|context| {
        context.write_u8(GuestAddress::new(0x1a890), 1).unwrap();
        context.write_u8(GuestAddress::new(0x20e8), 7).unwrap();
        context.write_u16(GuestAddress::new(0x20ea), 1000).unwrap();
        assert_eq!(helper_dcc01c(context, ExposureSelection(0)), Ok(false));
        let text = context.effects.text.unwrap();
        assert_eq!(&text[..14], b"OPT:007  ELE:\0");
        assert_eq!(&text[14..17], &[0x5a; 3]);
    });
}
