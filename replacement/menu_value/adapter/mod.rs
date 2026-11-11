//! The owned value transaction at the stock commit dispatcher's IMAGE SIZE
//! case arm: the jump-table word for property `0xf6` at `0x00d50084` (stock
//! `0x00d51b58`) points at `rb_menu_value_size_arm`, which returns to the stock
//! arm or to the shared tail `0x00d52674`. Every other property keeps its stock
//! word.

use crate::menu_value::{
    resolve, ChoiceCode, Commit, PropertyId, RetainedEffect, RowToken, Settings, Transaction, ValueTarget,
};

use core::ptr::read_volatile;

include!(env!("RB_MENU_VALUE_TABLE"));

const SIZE_CELL: u32 = 0x0095_6888;
const ASPECT_CELL: u32 = 0x0095_6898;
const THIRD_CELL: u32 = 0x0095_6890;
const STORED_SIZE: u32 = 0x0095_6c70;
const STORED_THIRD: u32 = 0x0095_6c78;
const STORED_ASPECT: u32 = 0x0095_6c80;
const MODE_CELL: u32 = 0x0095_6838;

fn display_updates() -> bool {
    unsafe { read_volatile(MODE_CELL as *const u32) != 0x2c }
}

fn cells_differ() -> bool {
    unsafe {
        crate::menu_value::cells_differ(
            [
                ChoiceCode::new(read_volatile(SIZE_CELL as *const u32)),
                ChoiceCode::new(read_volatile(ASPECT_CELL as *const u32)),
                ChoiceCode::new(read_volatile(THIRD_CELL as *const u32)),
            ],
            [
                ChoiceCode::new(read_volatile(STORED_SIZE as *const u32)),
                ChoiceCode::new(read_volatile(STORED_ASPECT as *const u32)),
                ChoiceCode::new(read_volatile(STORED_THIRD as *const u32)),
            ],
        )
    }
}

unsafe extern "C" {
    fn rb_menu_value_call_size_cell(value: u32);
    fn rb_menu_value_call_aspect_cell(value: u32);
    fn rb_menu_value_call_property_read(property: u32, flags: u32) -> u32;
    fn rb_menu_value_call_property_write(property: u32, value: u32, flags: u32);
    fn rb_menu_value_call_current_property(kind: u32, reserved: u32, property: u32);
    fn rb_menu_value_call_display_update(family: u32, reserved: u32);
    fn rb_menu_value_call_change_marker(flag: u32);
}

struct Stock;

impl Settings for Stock {
    fn read(&self, property: PropertyId) -> ChoiceCode {
        ChoiceCode::new(unsafe { rb_menu_value_call_property_read(property.get() as u32, 0) })
    }

    fn write(&mut self, property: PropertyId, value: ChoiceCode) {
        unsafe { rb_menu_value_call_property_write(property.get() as u32, value.get(), 0) }
    }
}

static mut TRANSACTION: Option<Transaction> = None;
static mut ENTRY_CELLS: [u32; 3] = [0; 3];

fn transaction() -> &'static mut Option<Transaction> {
    unsafe { &mut *core::ptr::addr_of_mut!(TRANSACTION) }
}

fn capture_cells() {
    unsafe {
        let cells = &mut *core::ptr::addr_of_mut!(ENTRY_CELLS);
        cells[0] = read_volatile(SIZE_CELL as *const u32);
        cells[1] = read_volatile(ASPECT_CELL as *const u32);
        cells[2] = read_volatile(THIRD_CELL as *const u32);
    }
}

fn restore_cells() {
    unsafe {
        let cells = *core::ptr::addr_of!(ENTRY_CELLS);
        core::ptr::write_volatile(SIZE_CELL as *mut u32, cells[0]);
        core::ptr::write_volatile(ASPECT_CELL as *mut u32, cells[1]);
        core::ptr::write_volatile(THIRD_CELL as *mut u32, cells[2]);
    }
}

fn apply(commit: &Commit, property: PropertyId) {
    let pair = commit.image_size;
    for effect in commit.retained.iter() {
        match effect {
            RetainedEffect::SizeCell => {
                if let Some(pair) = pair {
                    unsafe { rb_menu_value_call_size_cell(pair.size.get()) }
                }
            }
            RetainedEffect::AspectCell => {
                if let Some(pair) = pair {
                    unsafe { rb_menu_value_call_aspect_cell(pair.aspect.get()) }
                }
            }
            _ => {}
        }
    }
    let updates = display_updates();
    for effect in commit.retained.iter() {
        match effect {
            RetainedEffect::CurrentPropertyMarker if updates => unsafe {
                rb_menu_value_call_current_property(0x34, 0, property.get() as u32)
            },
            RetainedEffect::DisplayUpdate if updates => unsafe { rb_menu_value_call_display_update(1, 0) },
            RetainedEffect::SizeFamilyChangeMarker => {
                if cells_differ() {
                    unsafe { rb_menu_value_call_change_marker(1) }
                }
            }
            RetainedEffect::SizeCell | RetainedEffect::AspectCell => {}
            RetainedEffect::CurrentPropertyMarker | RetainedEffect::DisplayUpdate => {}
        }
    }
}

fn write_all(commit: &Commit) {
    let mut settings = Stock;
    for write in commit.writes.iter() {
        settings.write(write.property, write.value);
    }
}

#[no_mangle]
pub extern "C" fn rb_menu_value_begin(property: u32) -> u32 {
    if transaction().is_some() {
        return 0;
    }
    let property = PropertyId::new(property as u16);
    let target = TABLES.target(property, RowToken::new(0));
    if !matches!(target, ValueTarget::ImageSize { .. }) {
        return 0;
    }
    match Transaction::begin(&TABLES, target, &Stock) {
        Ok(begin) => {
            *transaction() = Some(begin);
            capture_cells();
            1
        }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn rb_menu_value_size_apply(property: u32, token: u32) -> u32 {
    #[cfg(rb_menu_value_always_decline)]
    {
        let _ = (property, token);
        return 0;
    }
    #[cfg(not(rb_menu_value_always_decline))]
    {
        let property = PropertyId::new(property as u16);
        let token = RowToken::new(token);
        let state = transaction();
        match state.as_mut() {
            Some(session) => match session.select(&TABLES, token, &mut Stock) {
                Ok(commit) => {
                    apply(&commit, property);
                    1
                }
                Err(_) => 0,
            },
            None => match resolve(&TABLES, TABLES.target(property, token)) {
                Ok(commit) => {
                    write_all(&commit);
                    apply(&commit, property);
                    1
                }
                Err(_) => 0,
            },
        }
    }
}

unsafe extern "C" {
    fn rb_menu_value_call_snapshot_cells(param: u32);
    fn rb_menu_value_call_snapshot_state(param: u32);
    fn rb_menu_value_call_restore_aux(clear: u32, aux: u32);
}

/// The retained part of `0x637ed0`: mirror and auxiliary snapshots and restore-auxiliary, without the settings copy `0x61c90c`.
#[no_mangle]
pub extern "C" fn rb_menu_value_retained(param: u32) {
    let aux = unsafe {
        read_volatile(if param == 0 { 0x0063_7f0c } else { 0x0063_7f10 } as *const u32)
    };
    unsafe {
        rb_menu_value_call_snapshot_cells(param);
        rb_menu_value_call_snapshot_state(param);
        rb_menu_value_call_restore_aux((param == 0) as u32, aux);
    }
}

#[no_mangle]
pub extern "C" fn rb_menu_value_cancel() -> u32 {
    match transaction().take() {
        Some(transaction) => {
            transaction.back(&mut Stock);
            restore_cells();
            1
        }
        None => 0,
    }
}

#[inline(never)]
#[no_mangle]
pub extern "C" fn rb_menu_value_confirm() -> u32 {
    transaction().take().is_some() as u32
}
