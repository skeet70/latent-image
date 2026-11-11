use crate::capture_profile_conversions_a as upper;
use crate::capture_profile_conversions_b as lower;
use crate::capture_profile_primary::{suppress_numeric_selection, ProfileOption};
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, HostAddressSpace, MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission};

fn descriptor(base: u32, length: u32) -> RegionDescriptor {
    RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
}

fn with_buffers<R>(buffer: &mut [u8], last_channel: &mut [u8; 4], body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>, GuestAddress) -> R) -> R {
    let mut regions = [
        MemoryRegion::new(descriptor(0x1000, buffer.len() as u32), buffer).unwrap(),
        MemoryRegion::new(descriptor(0x0091_34d8, 4), last_channel).unwrap(),
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    body(&mut RecordContext::new(memory, ()), GuestAddress::new(0x1000))
}

fn pure_context<R>(body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
    let mut regions = [];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    body(&mut RecordContext::new(memory, ()))
}

#[test]
fn native_selection_flags_preserve_only_the_three_permitted_bits() {
    pure_context(|context| {
        assert_eq!(lower::helper_736694(context, 0x2000, 2, 1, 0), Ok(1));
        assert_eq!(lower::helper_736694(context, 0x0400, 2, 1, 0), Ok(2));
        assert_eq!(lower::helper_736694(context, 0x4000, 2, 1, 0), Ok(4));
        assert_eq!(lower::helper_736694(context, u32::MAX, 2, 1, 0), Ok(7));
        for (category, enabled, override_value) in [(1, 1, 0), (44, 1, 0), (2, 0, 0), (2, 1, 1)] {
            assert_eq!(lower::helper_736694(context, u32::MAX, category, enabled, override_value), Ok(0));
        }
    });
}

#[test]
fn category_variant_and_extended_profiles_keep_distinct_codes() {
    pure_context(|context| {
        assert_eq!(lower::helper_7362dc(context, 29, 0, 0, 0, 0), Ok(53));
        assert_eq!(lower::helper_7362dc(context, 29, 0, 0, 0, 3), Ok(56));
        assert_eq!(lower::helper_7362dc(context, 33, 0, 0, 1, 0), Ok(57));
        assert_eq!(lower::helper_7362dc(context, 33, 0, 0, 2, 0), Ok(44));
        assert_eq!(lower::helper_7362dc(context, 12, 1, 0, 0, 0), Ok(13));
        assert_eq!(lower::helper_7362dc(context, 12, 1, 1, 0, 0), Ok(12));
        assert_eq!(lower::helper_7362dc(context, u32::MAX, 0, 0, 0, 0), Ok(0));
    });
}

#[test]
fn signed_color_adjustment_handles_both_boundaries_and_outside_values() {
    pure_context(|context| {
        for (input, expected) in [(-10i32, 9), (-9, 0), (0, 9), (9, 18), (10, 9)] {
            assert_eq!(lower::helper_737378(context, input as u32), Ok(expected));
        }
    });
}

#[test]
fn percentage_and_scaled_value_selections_keep_native_defaults() {
    pure_context(|context| {
        assert_eq!(lower::helper_7380fc(context, 1), Ok(33));
        assert_eq!(lower::helper_7380fc(context, 2), Ok(66));
        assert_eq!(lower::helper_7380fc(context, 3), Ok(100));
        assert_eq!(lower::helper_7380fc(context, 4), Ok(0));
        assert_eq!(lower::helper_738204(context, 0), Ok(2500));
        assert_eq!(lower::helper_738204(context, 1), Ok(2550));
        assert_eq!(lower::helper_738204(context, 30), Ok(10000));
        assert_eq!(lower::helper_738204(context, 31), Ok(2500));
    });
}

#[test]
fn secondary_channel_selection_publishes_and_retains_native_bytes() {
    let mut buffer = [0u8; 28];
    let mut last_channel = [0u8; 4];
    with_buffers(&mut buffer, &mut last_channel, |context, output| {
        let words = |context: &mut RecordContext<HostAddressSpace<'_, '_>, ()>| {
            let mut values = [0u32; 7];
            for (index, value) in values.iter_mut().enumerate() {
                *value = context.read_u32(GuestAddress::new(0x1000 + index as u32 * 4)).unwrap();
            }
            values
        };
        assert_eq!(upper::helper_737aa8(context, output, 1, 0x43, 123), Ok(0x1000));
        assert_eq!(words(context), [4, 4, 1, 0, 0, 123, 0]);
        assert_eq!(context.read_u32(GuestAddress::new(0x0091_34d8)), Ok(4));
        assert_eq!(upper::helper_737aa8(context, output, 0, 0, 999), Ok(0x1000));
        assert_eq!(words(context), [4, 0, 1, 0, 0, 0, 0]);
        for (index, value) in [11u32, 22, 33, 44, 55, 66, 77].into_iter().enumerate() {
            context.write_u32(GuestAddress::new(0x1000 + index as u32 * 4), value).unwrap();
        }
        assert_eq!(upper::helper_737aa8(context, output, 1, 0x40, 999), Ok(0x1000));
        assert_eq!(words(context), [0, 0, 1, 44, 55, 66, 77]);
        for (index, value) in [11u32, 22, 33, 44, 55, 66, 77].into_iter().enumerate() {
            context.write_u32(GuestAddress::new(0x1000 + index as u32 * 4), value).unwrap();
        }
        assert_eq!(upper::helper_737aa8(context, output, 2, 0x41, 999), Ok(0x1000));
        assert_eq!(words(context), [0, 0, 33, 44, 55, 66, 77]);
    });
}

#[test]
fn signed_channel_offsets_publish_word_and_byte_widths() {
    let mut buffer = [0u8; 8];
    let mut last_channel = [0u8; 4];
    with_buffers(&mut buffer, &mut last_channel, |context, output| {
        assert_eq!(upper::helper_737278(context, output, (-4i32) as u32, 5), Ok(0x1000));
        assert_eq!(context.read_u32(output), Ok(u32::MAX));
        assert_eq!(context.read_u32(GuestAddress::new(0x1004)), Ok((-2i32) as u32));
        assert_eq!(upper::helper_73845c(context, output, (-4i32) as u32, 5), Ok(0x1000));
        assert_eq!(context.read_u8(output), Ok(255));
        assert_eq!(context.read_u8(GuestAddress::new(0x1001)), Ok(254));
    });
}

#[test]
fn secondary_enabled_code_and_category_gates_are_independent() {
    pure_context(|context| {
        assert_eq!(upper::helper_73790c(context, 1, 0xd0), Ok(1));
        assert_eq!(upper::helper_73790c(context, 1, 0xd1), Ok(2));
        assert_eq!(upper::helper_73790c(context, 1, 0xd9), Ok(3));
        assert_eq!(upper::helper_73790c(context, 2, 0xd1), Ok(0));
        assert_eq!(upper::helper_7379a0(context, 0, 44, 0), Ok(0));
        assert_eq!(upper::helper_7379a0(context, 0, 44, 1), Ok(1));
        assert_eq!(upper::helper_7379a0(context, 1, 44, 2), Ok(0));
        assert_eq!(upper::helper_7379a0(context, 0, 43, 0), Ok(1));
    });
}

#[test]
fn numeric_profile_suppression_requires_exact_override_value_one() {
    for category in [1, 5, 8, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 29, 32, 44] {
        assert!(suppress_numeric_selection(category, ProfileOption(0), ProfileOption(0)));
    }
    for category in [0, 2, 3, 4, 6, 7, 9, 10, 14, 15, 27, 28, 30, 31, 33, 43, 45, u32::MAX] {
        assert!(!suppress_numeric_selection(category, ProfileOption(0), ProfileOption(0)));
        assert!(suppress_numeric_selection(category, ProfileOption(1), ProfileOption(0)));
        assert!(suppress_numeric_selection(category, ProfileOption(0), ProfileOption(1)));
        assert!(!suppress_numeric_selection(category, ProfileOption(2), ProfileOption(2)));
    }
}

#[test]
fn selection_five_or_six_predicate_rejects_adjacent_and_wrapped_inputs() {
    pure_context(|context| {
        for (value, expected) in [(0, 0), (4, 0), (5, 1), (6, 1), (7, 0), (u32::MAX, 0)] {
            assert_eq!(upper::helper_738578(context, value), Ok(expected));
        }
    });
}
