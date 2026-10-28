//! The packaged bank's retained row-availability service call.
//!
//! `FUN_00d52684` answers, for a stock filter-list property and a row token,
//! whether that row is offered. Edit-field hosts `0x267..0x26f` reach it with
//! the stock property they edit; the C lane answers every other property.

/// `FUN_00d52684`'s row states, as the stock list machinery consumes them
/// (menu/recipe_bank_ui.h).
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Active = 0,
    Disabled = 2,
    Hidden = 3,
}

/// The retained service's answer; `Unsupported` carries the word unchanged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowAnswer {
    Supported(RowState),
    Unsupported(u32),
}

impl RowAnswer {
    fn parse(word: u32) -> Self {
        match word {
            0 => RowAnswer::Supported(RowState::Active),
            2 => RowAnswer::Supported(RowState::Disabled),
            3 => RowAnswer::Supported(RowState::Hidden),
            other => RowAnswer::Unsupported(other),
        }
    }

    fn word(self) -> u32 {
        match self {
            RowAnswer::Supported(state) => state as u32,
            RowAnswer::Unsupported(word) => word,
        }
    }
}

unsafe extern "C" {
    /// AAPCS: preserves r4-r11 and sp, clobbers r0-r3, r12 and lr.
    fn rb_call_stock_probe(field: u32, row: u32) -> u32;
}

/// A word outside `RowState` keeps the caller's conversion of the stock word.
#[no_mangle]
pub unsafe extern "C" fn rb_service_field_row_state(field: u32, row: u32) -> u32 {
    let word = unsafe { rb_call_stock_probe(field, row) };
    RowAnswer::parse(word).word()
}

