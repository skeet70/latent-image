use super::*;

#[derive(Clone, Copy)]
struct SelectionCategory(u8);
#[derive(Clone, Copy)]
struct SelectionBank { row: u8, column: u8 }
#[derive(Clone, Copy)]
enum Transfer { Geometry, Complete }

unsafe fn publish_geometry(item: Item, slot: Slot, record: Address, saved: Address) {
    let p = record.0;
    let s = i32::from(slot.0);
    h7529e8(item.0, s, i32::from(half(p)));
    h752a98(item.0, s, word(p + 4));
    h752bf8(item.0, s, i32::from(half(p + 10)));
    h752ca8(item.0, s, i32::from(half(p + 12)));
    h752d58(item.0, s, i32::from(half(p + 14)));
    h752b48(item.0, s, i32::from(half(p + 8)));
    h753648(item.0, s, word(p + 0x28));
    h7532d8(item.0, s, i32::from(byte(p + 0x2c)));
    h753178(item.0, s, i32::from(half(p + 0x1a)));
    h753388(item.0, s, word(p + 0x1c));
    h753438(item.0, s, word(p + 0x20));
    h7534e8(item.0, s, word(p + 0x24));
    h753228(item.0, s, i32::from(half(p + 0x18)));
    h752f68(item.0, s, i32::from(half(p + 0x10)));
    h753018(item.0, s, i32::from(half(p + 0x12)));
    h7530c8(item.0, s, i32::from(half(p + 0x14)));
    h753598(item.0, s, i32::from(half(p + 0x16)));
    h753f38(item.0, s, i32::from(half(p + 0x4c)));
    h753fe8(item.0, s, i32::from(half(p + 0x4e)));
    h7544b8(item.0, s, word(LIST + 0x1eec + usize::from(slot.0) * 4));
    h754568(item.0, s, word(saved.0 + usize::from(slot.0) * 4));
}

unsafe fn transfer(item: Item, route: i32, mode: i32, request_flag: i32, special: i32, transfer: Transfer) {
    let bank_list = if route == 1 { 0x016b3cb0 } else { 0x016b3cc8 };
    let category = SelectionCategory(byte(PROFILE + 0x6414));
    let category_offset = usize::from(category.0);
    let mut slot = Slot(u8::from(special == 1 && f2f460() == 1));
    while u32::from(slot.0) < word(PROFILE + 0x5c) as u32 {
        let index = usize::from(slot.0);
        let list_bank = if word(PROFILE + 0x6568) == 0 { 7 } else { byte(bank_list + index * 4) };
        let pair = f1b050(7, i32::from(category.0), i32::from(slot.0)) as u32 as usize;
        let bank = SelectionBank { row: byte(pair), column: byte(pair + 1) };
        let row = usize::from(bank.row);
        let column = usize::from(bank.column);
        let geometry_offset = row * 0x2f4 + column * 0xfc;
        let mut destination = 0x016ad7a0 + geometry_offset;
        let mut source = 0x016aedc8 + geometry_offset + category_offset * 0x8dc;
        let mut saved = 0x016b3bac + category_offset * 0xc;
        if list_bank < 6 && word(PROFILE + 0x6568) != 0 {
            copy(destination + index * 0x54, source + index * 0x54, 0x54);
            destination = 0x016ae448 + usize::from(list_bank) * 0xfc;
            source = 0x016b13c0 + usize::from(list_bank) * 0x2f4 + category_offset * 0xfc;
            saved = 0x016b3bd0 + usize::from(list_bank) * 0x24 + category_offset * 0xc;
        }
        let record = Address(destination + index * 0x54);
        copy(record.0, source + index * 0x54, 0x54);
        let word_offset = row * 0x24 + column * 0xc;
        let half_offset = row * 0x12 + column * 6;
        let words = 0x016aea30 + word_offset;
        let halves = 0x016aea9c + half_offset;
        let packed_offset = row * 0xcc + column * 0x44 + index * 0x10;
        if matches!(transfer, Transfer::Complete) {
            put(words + index * 4, word(0x016b2578 + word_offset + category_offset * 0x6c + index * 4));
            put_half(halves + index * 2, half(0x016b26bc + half_offset + category_offset * 0x36 + index * 2));
            copy(0x016b3198 + packed_offset, 0x016b33fc + packed_offset + category_offset * 0x264, 16);
        }
        let mut selection = [0i32; 2];
        let mut channel = 0i32;
        let mut aperture = 0i16;
        let result = f1d6a0(selected().0, mode, request_flag, word(record.0 + 4), i32::from(slot.0),
            selection.as_mut_ptr() as i32, &mut channel as *mut i32 as i32, &mut aperture as *mut i16 as i32);
        if slot.0 == 0 { put(PROFILE + 0x65f0, result); }
        if result == 2 {
            h752a98(item.0, i32::from(slot.0), channel);
            h752b48(item.0, i32::from(slot.0), i32::from(aperture));
            f1d5d4(selection[0], i32::from(category.0), i32::from(bank.row), i32::from(bank.column),
                i32::from(slot.0), i32::from(list_bank), channel, i32::from(aperture));
        } else {
            publish_geometry(item, slot, record, Address(saved));
            if matches!(transfer, Transfer::Complete) {
                let value = f60b18(word(words + index * 4));
                h7536f8(item.0, i32::from(slot.0), value);
                h7537a8(item.0, i32::from(slot.0), i32::from(half(halves + index * 2)));
                let value = f60b48(word(words + index * 4));
                h753858(item.0, i32::from(slot.0), value);
                let value = f60b60(word(words + index * 4));
                h753908(item.0, i32::from(slot.0), value);
                h7539b8(item.0, i32::from(slot.0), word(0x016b319c + packed_offset));
                let value = f60adc(i32::from(byte(0x016b3198 + packed_offset)));
                h753a68(item.0, i32::from(slot.0), value);
                h753b18(item.0, i32::from(slot.0), 0);
                h753bc8(item.0, i32::from(slot.0), 0);
                h7542a8(item.0, i32::from(slot.0), word(0x016b343c + row * 0xcc + column * 0x44 + category_offset * 0x264));
            }
        }
        slot.0 = slot.0.wrapping_add(1);
    }
    if mode == 7 && (word(PROFILE + 0x65f0) as u32) < 2 {
        f1d134((PROFILE + 4) as i32);
        f17af8(1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f18b94(item: i32, route: i32, mode: i32, flag: i32, special: i32) {
    transfer(Item(item), route, mode, flag, special, Transfer::Complete);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f19204(item: i32, route: i32, mode: i32, flag: i32, special: i32) {
    transfer(Item(item), route, mode, flag, special, Transfer::Geometry);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_758940(destination: *mut u8) -> *mut u8 {
    copy(destination as usize, 0x00919860, 36);
    destination
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_758980(raw_item: i32, source: *const u8) {
    let item = Item(raw_item);
    if h75a8c0(item.0, 34) != 0 { copy(0x00919860, source as usize, 36); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_75890c(raw_item: i32, source: *const u8) {
    let item = Item(raw_item);
    if h75a8c0(item.0, 33) != 0 { copy(0x009197ec, source as usize, 76); }
}
