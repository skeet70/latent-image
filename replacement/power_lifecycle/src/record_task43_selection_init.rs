#[path = "record_task43_selection_init_boundary.rs"]
mod boundary;
#[path = "record_task43_selection_init_fields.rs"]
mod fields;
#[path = "record_task43_selection_init_diagnostics.rs"]
mod diagnostics;
use boundary::*;

core::arch::global_asm!(include_str!("record_task43_selection_init_residue.S"), options(raw));

#[derive(Clone, Copy)]
enum Initialization { Owner, Single }
impl Initialization {
    fn from_raw(value: u32) -> Self { if value == 0 { Self::Owner } else { Self::Single } }
    fn is_owner(self) -> bool { matches!(self, Self::Owner) }
}

#[unsafe(export_name = "xm1_power_record_selection_initialize")]
pub unsafe extern "C" fn initialize_selection(raw_mode: u32, reset_tail: u16) {
    unsafe {
        let mode = Initialization::from_raw(raw_mode);
        xm1_power_recording_capture_trace_begin(0x02000000);
        let (value, special) = if mode.is_owner() {
            let current = Address(ef9bc0());
            CONTEXT.offset(4).set(current.0);
            let value = current.offset(0x30).word();
            let operation = current.offset(4).word();
            if matches!(operation, 0 | 5 | 7) { f4727c(); }
            let current = owner();
            if resource5a(profile().0) == 0 && current.offset(4).word() == 0 { f472bc(); }
            (value, operation == 6)
        } else {
            CONTEXT.offset(4).set(0);
            (Address(ef9b50()).offset(0x14).word(), false)
        };
        let mut retain_lists = true;
        if !special {
            runtime_mode(4);
            runtime_clear(1, 0);
            f160ac(value);
            f16160(0);
            f161f0();
            initialize_geometry();
            if mode.is_owner() {
                let mut state = SelectionState::default();
                let mut header = SelectionHeader::default();
                let mut statistics = SelectionStatistics::default();
                read_state(profile().0, &mut state);
                read_header(profile().0, &mut header);
                read_statistics(&mut statistics);
                retain_lists = state.words[0] == f160d0();
                if !retain_lists {
                    state.words[1] = 0;
                    state.words[24] = 0;
                    write_state(profile().0, &state);
                    statistics.words[0] = 0;
                    write_statistics(profile().0, &statistics);
                }
                if header.words[0] != 0 && header.words[3] != f160d0() {
                    header.words[0] = 0;
                    write_header(profile().0, &header);
                }
            }
        }
        let enabled = f16664();
        if enabled != 0 && !special {
            f162e4(0x16a026c, profile().0, owner().offset(0x18).word(), 0, raw_mode);
        }
        f15f40(u32::from(mode.is_owner() && enabled != 0 && retain_lists));
        f1572c(raw_mode, owner().offset(4).word());
        if !special {
            f1608c();
            let mut scratch = [0u32; 16];
            read_scratch(profile().0, scratch.as_mut_ptr());
            scratch.fill(0);
            write_scratch(profile().0, scratch.as_ptr());
            f1fd84();
            ee8e70(0x16a048c);
        }
        let reset = [0, 0, 0, 0, 0, u32::from(reset_tail) << 16];
        f2f400(reset.as_ptr());
        f2f480(0);
        f2f440(0);
        if mode.is_owner() && owner().offset(4).word() == 2 {
            let pair = mode_pair(profile().0);
            if pair != 0x00040003 && pair != 0x00030002 {
                let mut state = SelectionState::default();
                read_state(profile().0, &mut state);
                state.words[1] = 0;
                state.words[24] = 0;
                write_state(profile().0, &state);
            }
        }
        if !special {
            let mut state = SelectionState::default();
            read_state(profile().0, &mut state);
            if owner().offset(4).word() == 1 && mode_flag(profile().0) == 1 {
                state.words[26] = 0;
                write_state(profile().0, &state);
            }
            if owner().offset(0x10).word() & 2 != 0 && state.words[32] != 0x80 {
                state.words[26] = 0;
                write_state(profile().0, &state);
            }
        }
        if !special && mode.is_owner() { owner().offset(0x2c).pointee().pointee().offset(0x63314).set(0); }
        clear_geometry_runtime();
        if mode.is_owner() { publish_owner_inputs(); }
        let mut published = 0;
        if owner().0 != 0 {
            let current = owner();
            let buffer_pointer = current.offset(0x2c).pointee();
            if buffer_pointer.0 != 0 && buffer_pointer.word() != 0 {
                let destination = buffer_pointer.pointee();
                destination.offset(0x35cc).set(current.offset(4).word());
                destination.offset(0x35d0).set(current.offset(0x10).word());
                destination.offset(0x35d4).set(current.offset(0xc).word());
                destination.offset(0x35d8).set(current.offset(0x30).word());
                Address(0x16cd23c).set(destination.offset(0x35c8).0);
                published = current.offset(0x2c).pointee().word();
            }
            if !special && mode.is_owner() {
                diagnostics::apply_owner_setting();
                diagnostics::restore_owner_buffer();
                diagnostics::apply_resource_mode();
                diagnostics::export_diagnostics();
                diagnostics::apply_active_owner();
                f4929c(buffer().0);
            }
        }
        if mode.is_owner() && matches!(owner().offset(4).word(), 0 | 3 | 5) { runtime_refresh(); }
        if resource42(profile().0) == 1 && !special && mode.is_owner() && resource43(profile().0) != 0x39 {
            publish_buffer(profile().0, &published);
        }
        clear_buffer(profile().0, 0);
        let operation = owner().offset(4).word();
        if operation == 7 {
            LISTS.offset(0x1f14).set(LISTS.offset(0x1f10).word());
            LISTS.offset(0x1f10).set(0);
        } else {
            LISTS.offset(0x1f10).set(0);
            LISTS.offset(0x1f14).set(0);
            if operation == 0 || operation == 5 { LISTS.offset(0x1f18).set(0); }
        }
    }
}

unsafe fn initialize_geometry() {
    unsafe {
        let item = profile();
        let mut geometry = Geometry::default();
        read_geometry(item.0, &mut geometry);
        geometry.words[0] = (geometry.words[0] & 0xffff0000) | (lens_position() & 0xffff);
        geometry.words[1] = f160d0();
        geometry.words[3] = lens_third();
        geometry.words[2] = lens_second();
        for index in 0..4 { geometry.words[index + 4] = geometry.words[index]; }
        geometry.words[8] = 0;
        write_geometry(item.0, &geometry);
        let item = profile();
        let current = owner();
        if current.0 == 0 || current.offset(0x24).word() == 0 { return; }
        read_geometry(item.0, &mut geometry);
        let mut alternate = AlternateGeometry::default();
        read_alternate(item.0, &mut alternate);
        let source = if alternate.words[0] == 1 { &alternate.words[3..7] } else { &geometry.words[..4] };
        let first = source[0];
        let second = source[1];
        let third = source[2];
        let fourth = source[3];
        for offset in [0xc, 0x1c] {
            let destination = current.offset(0x24).pointee().offset(offset);
            for (index, value) in source.iter().enumerate() { destination.offset(index as u32 * 4).set(*value); }
            destination.set_half(first as u16);
            destination.offset(4).set(second);
            destination.offset(8).set(third);
            destination.offset(12).set(fourth);
        }
        let destination = current.offset(0x24).pointee();
        destination.offset(0x1c).set_half(f4725c(second, third, fourth) as u16);
        let destination = current.offset(0x24).pointee();
        let flags = lens_flags();
        destination.offset(4).set(geometry_value(first as i16 as i32, second, third, flags, fourth));
    }
}

unsafe fn publish_owner_inputs() {
    unsafe {
        if owner().offset(0x10).word() & 1 == 0 { set_owner_value(profile().0, owner().offset(0x1c).word()); }
        if owner().offset(0x10).word() & 2 == 0 {
            let mut index = 0;
            while index < owner().offset(0x18).pointee().word() {
                let value = owner().offset(0x20).pointee().offset(index * 4).word();
                set_owner_record(profile().0, value, index);
                index += 1;
            }
        }
        let flags = owner().offset(0x10).word();
        if flags & 4 == 0 && flags & 8 != 0 { set_owner_geometry(profile().0, owner().offset(0x24).word()); }
    }
}
