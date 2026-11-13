//! The owned IMAGE SIZE commit at the stock commit dispatcher's case arm:
//! the jump-table word for property `0xf6` at `0x00d50084` (stock
//! `0x00d51b58`) points at `rb_menu_value_size_arm`, which returns to the stock
//! arm or to the shared tail `0x00d52674`. Every other property keeps its stock
//! word.

use crate::menu_value::{
    resolve, ChoiceCode, Commit, PropertyId, RetainedEffect, RowToken, Settings,
};

use core::ptr::read_volatile;
use xm1_property_service::{Properties, PropertyId as ServicePropertyId, SettingsBank, ValueBits, WriteValue};

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
    fn rb_menu_value_call_current_property(kind: u32, reserved: u32, property: u32);
    fn rb_menu_value_call_display_update(family: u32, reserved: u32);
    fn rb_menu_value_call_change_marker(flag: u32);
}

struct Stock;

impl Settings for Stock {
    fn write(&mut self, property: PropertyId, value: ChoiceCode) {
        if let Some(id) = ServicePropertyId::decode(property.get() as u32) {
            let mut properties = unsafe { Properties::stock() };
            properties.write(id, WriteValue::Value(ValueBits::new(value.get())), SettingsBank::Active);
        }
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

#[no_mangle]
pub extern "C" fn rb_menu_value_size_apply(property: u32, token: u32) -> u32 {
    let Ok(property) = u16::try_from(property) else {
        return 0;
    };
    let property = PropertyId::new(property);
    let token = RowToken::new(token);
    match resolve(&TABLES, TABLES.target(property, token)) {
        Ok(commit) => {
            commit.write_to(&mut Stock);
            apply(&commit, property);
            1
        }
        Err(_) => 0,
    }
}
