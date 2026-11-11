use crate::record_address_space::*;
use crate::record_image_command_results::*;
use crate::record_image_command_resources::*;
use crate::shutdown::KernelStatus;

#[derive(Default)]
struct Cache { cleaned: Vec<(GuestAddress, ByteCount)> }
impl ImageCache for Cache {
    fn clean_cache(&mut self, address: GuestAddress, length: ByteCount) -> Result<(), KernelStatus> {
        self.cleaned.push((address, length));
        Ok(())
    }
}

fn with_context(test: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Cache>)) {
    let mut buffers = [
        vec![0; 0x5000], vec![0; 0x1500], vec![0; 0x1500], vec![0; 4],
        vec![0; 72], vec![0; 4], vec![0; 0x2200], vec![0; 1],
    ];
    let bases = [0x1000, 0x7000, 0x000e_bf58, 0x0096_ca50,
        0x0170_32d4, 0x0170_3ad4, 0x0170_5784, 0x4171_bc28];
    let mut regions: Vec<_> = buffers.iter_mut().zip(bases).map(|(bytes, base)| {
        let descriptor = RegionDescriptor::new(GuestAddress::new(base), ByteCount(bytes.len() as u32),
            RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        MemoryRegion::new(descriptor, bytes).unwrap()
    }).collect();
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    test(&mut RecordContext::new(memory, Cache::default()));
}

#[test]
fn capture_history_exports_unsigned_bytes_and_missing_entries() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x0170_32d4), 2).unwrap();
        context.write_u8(GuestAddress::new(0x0170_32dc), 0x81).unwrap();
        context.write_u8(GuestAddress::new(0x0170_32dd), 0xfe).unwrap();
        helper_103b044(context, GuestAddress::new(0x1000)).unwrap();
        assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(0x5354_4643));
        assert_eq!(context.read_u32(GuestAddress::new(0x1004)), Ok(0x81));
        assert_eq!(context.read_u32(GuestAddress::new(0x1008)), Ok(0xfe));
        for address in (0x100c..0x1104).step_by(4) {
            assert_eq!(context.read_u32(GuestAddress::new(address)), Ok(u32::MAX));
        }
        assert_eq!(context.read_u32(GuestAddress::new(0x1104)), Ok(0x4c41_4643));
    });
}

#[test]
fn capture_history_rotates_at_the_next_write_position() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x0170_32d4), 63).unwrap();
        context.write_u32(GuestAddress::new(0x0170_32d8), 1).unwrap();
        for index in 0..64 { context.write_u8(GuestAddress::new(0x0170_32dc + index), index as u8).unwrap(); }
        helper_1036a70(context, GuestAddress::new(0x1000)).unwrap();
        assert_eq!(context.read_u32(GuestAddress::new(0x1004)), Ok(63));
        assert_eq!(context.read_u32(GuestAddress::new(0x1008)), Ok(0));
        assert_eq!(context.read_u32(GuestAddress::new(0x1100)), Ok(62));
    });
}

#[test]
fn preview_configuration_truncates_fields_without_replacing_adjacent_state() {
    with_context(|context| {
        context.write_u8(GuestAddress::new(0x0170_5896), 0xee).unwrap();
        for index in 0..14 { context.write_u8(GuestAddress::new(0x2000 + index), index as u8 + 20).unwrap(); }
        helper_1042e9c(context, PreviewConfiguration {
            handle: 123, table: GuestAddress::new(0x2000), kind: 0x123,
            width: 0x12345, height: 0x23456, x: 0x34567, y: 0x45678,
            first: 0x56789, second: 0x6789a,
        }).unwrap();
        assert_eq!(context.read_u32(GuestAddress::new(0x0170_5784)), Ok(123));
        assert_eq!(context.read_u8(GuestAddress::new(0x0170_589a)), Ok(0x8c));
        assert_eq!(context.read_u32(GuestAddress::new(0x0170_58a8)), Ok(0x3456_2345));
        assert_eq!(context.read_u32(GuestAddress::new(0x0170_58ac)), Ok(0x5678_4567));
        assert_eq!(context.read_u32(GuestAddress::new(0x0170_58b0)), Ok(0x789a_6789));
        for index in 0..14 { assert_eq!(context.read_u8(GuestAddress::new(0x0170_5888 + index)), Ok(index as u8 + 20)); }
        assert_eq!(context.read_u8(GuestAddress::new(0x0170_5896)), Ok(0xee));
    });
}

#[test]
fn result_refresh_rejects_null_and_leaves_disabled_output_untouched() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x0096_ca50), 0x2000).unwrap();
        context.write_u32(GuestAddress::new(0x1000), 0x12345678).unwrap();
        assert_eq!(helper_10517f8(context, GuestAddress::new(0)), Ok(132));
        assert_eq!(helper_10517f8(context, GuestAddress::new(0x1000)), Ok(0));
        assert_eq!(context.read_u32(GuestAddress::new(0x1000)), Ok(0x12345678));
        assert!(context.effects.cleaned.is_empty());
    });
}

#[test]
fn enabled_refresh_exports_sixty_records_in_ring_order_and_flushes_source() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x0096_ca50), 0x2000).unwrap();
        context.write_u8(GuestAddress::new(0x20a8), 0x80).unwrap();
        context.write_u8(GuestAddress::new(0x4171_bc28), 59).unwrap();
        for index in 0..60 {
            context.write_u32(GuestAddress::new(0x0170_5918 + index * 68), 1000 + index).unwrap();
        }
        assert_eq!(helper_10517f8(context, GuestAddress::new(0x3000)), Ok(0));
        assert_eq!(context.read_u32(GuestAddress::new(0x3000)), Ok(0x5354_4348));
        assert_eq!(context.read_u32(GuestAddress::new(0x3038)), Ok(1059));
        assert_eq!(context.read_u32(GuestAddress::new(0x307c)), Ok(1000));
        assert_eq!(context.read_u32(GuestAddress::new(0x3fe4)), Ok(1058));
        assert_eq!(context.read_u32(GuestAddress::new(0x4028)), Ok(0x4c41_4348));
        assert_eq!(context.effects.cleaned, [(GuestAddress::new(0x0170_58e0), ByteCount(4160))]);
    });
}

#[test]
fn resource_export_writes_only_the_selected_snapshot_interval() {
    with_context(|context| {
        context.write_u32(GuestAddress::new(0x0096_ca50), 0x2000).unwrap();
        context.write_u8(GuestAddress::new(0x20a8), 0x80).unwrap();
        context.write_u32(GuestAddress::new(0x0170_3ad4), 0x7000).unwrap();
        for offset in (0..0x1500).step_by(4) {
            context.write_u32(GuestAddress::new(0x7000 + offset), offset ^ 0x1234_abcd).unwrap();
        }
        let destination = 0x000e_bf58;
        context.write_u32(GuestAddress::new(destination + 0x24c), 0xfedc_ba98).unwrap();
        context.write_u32(GuestAddress::new(destination + 0x14bc), 0x7654_3210).unwrap();
        helper_103b054(context, GuestAddress::new(0x10000)).unwrap();
        assert_eq!(context.read_u32(GuestAddress::new(destination + 0x24c)), Ok(0xfedc_ba98));
        assert_eq!(context.read_u32(GuestAddress::new(destination + 0x14bc)), Ok(0x7654_3210));
        for offset in (0x250..0x14bc).step_by(4) {
            assert_eq!(context.read_u32(GuestAddress::new(destination + offset)), Ok(offset ^ 0x1234_abcd));
        }
    });
}

#[test]
fn unmapped_result_output_is_not_reported_as_success() {
    with_context(|context| {
        let address = GuestAddress::new(0x6000);
        assert_eq!(helper_103b044(context, address), Err(MemoryError::Unmapped(address)));
    });
}
