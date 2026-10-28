//! Read-only classification of the recipe bank's menu pages and row tokens.
//!
//! No service calls and no stores.

#![cfg_attr(not(test), no_std)]

pub const VERBS_PER_SLOT: u32 = 4;
pub const BANK_SLOTS: u32 = 7;
pub const SELECT_ROWS: u32 = 8;

pub const CLASS_SLOT: u32 = 0x281;
pub const CLASS_SUB0: u32 = 0x270;
pub const CLASS_SELECT: u32 = 0x285;
pub const CLASS_EDITOR: u32 = 0x131;
pub const CLASS_EDIT: u32 = 0x277;
pub const CLASS_DIALOG: u32 = 0x27d;
pub const CLASS_FIELD0: u32 = 0x267;
pub const CLASS_FIELD_LAST: u32 = 0x26f;

pub const TOKEN_VERB_BASE: u32 = 0x40;
pub const TOKEN_SELECT_BASE: u32 = 0x60;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankPage {
    None = 0,
    Slot = 1,
    Sub = 2,
    Select = 3,
    Editor = 4,
    Edit = 5,
    Dialog = 6,
    Field = 7,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    None = 0,
    Verb = 1,
    Select = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankVerb {
    SaveCurrent = 0,
    EditName = 1,
    Edit = 2,
    Reset = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageRoute {
    pub page: BankPage,
    pub slot: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemRef {
    pub kind: ItemKind,
    pub slot: u8,
    pub verb: BankVerb,
    pub selection: u8,
}

const NO_ITEM: ItemRef = ItemRef {
    kind: ItemKind::None,
    slot: 0,
    verb: BankVerb::SaveCurrent,
    selection: 0,
};

pub fn route_page(cls: u32) -> PageRoute {
    if cls == CLASS_SLOT {
        return PageRoute { page: BankPage::Slot, slot: 0 };
    }
    if cls >= CLASS_SUB0 && cls < CLASS_SUB0 + BANK_SLOTS {
        return PageRoute {
            page: BankPage::Sub,
            slot: (cls - CLASS_SUB0) as u8,
        };
    }
    let page = if cls == CLASS_SELECT {
        BankPage::Select
    } else if cls == CLASS_EDITOR {
        BankPage::Editor
    } else if cls == CLASS_EDIT {
        BankPage::Edit
    } else if cls == CLASS_DIALOG {
        BankPage::Dialog
    } else if cls >= CLASS_FIELD0 && cls <= CLASS_FIELD_LAST {
        BankPage::Field
    } else {
        BankPage::None
    };
    PageRoute { page, slot: 0 }
}

pub fn decode_token(token: u32) -> ItemRef {
    if token >= TOKEN_VERB_BASE && token < TOKEN_VERB_BASE + BANK_SLOTS * VERBS_PER_SLOT {
        let offset = token - TOKEN_VERB_BASE;
        return ItemRef {
            kind: ItemKind::Verb,
            slot: (offset / VERBS_PER_SLOT) as u8,
            verb: match offset % VERBS_PER_SLOT {
                0 => BankVerb::SaveCurrent,
                1 => BankVerb::EditName,
                2 => BankVerb::Edit,
                _ => BankVerb::Reset,
            },
            selection: 0,
        };
    }
    if token >= TOKEN_SELECT_BASE && token < TOKEN_SELECT_BASE + SELECT_ROWS {
        return ItemRef {
            kind: ItemKind::Select,
            slot: 0,
            verb: BankVerb::SaveCurrent,
            selection: (token - TOKEN_SELECT_BASE) as u8,
        };
    }
    NO_ITEM
}

#[no_mangle]
pub extern "C" fn rb_ui_route_page(cls: u32) -> u32 {
    route_page(cls).page as u32
}

#[no_mangle]
pub extern "C" fn rb_ui_route_slot(cls: u32) -> u32 {
    route_page(cls).slot as u32
}

/// C ABI: `kind | slot << 8 | verb << 16 | selection << 24`.
#[no_mangle]
pub extern "C" fn rb_ui_route_item(token: u32) -> u32 {
    let item = decode_token(token);
    item.kind as u32
        | ((item.slot as u32) << 8)
        | ((item.verb as u32) << 16)
        | ((item.selection as u32) << 24)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(cls: u32) -> (BankPage, u8) {
        let route = route_page(cls);
        (route.page, route.slot)
    }

    #[test]
    fn classes_route_to_their_pages() {
        assert_eq!(page(CLASS_SLOT), (BankPage::Slot, 0));
        assert_eq!(page(CLASS_SELECT), (BankPage::Select, 0));
        assert_eq!(page(CLASS_EDITOR), (BankPage::Editor, 0));
        assert_eq!(page(CLASS_EDIT), (BankPage::Edit, 0));
        assert_eq!(page(CLASS_DIALOG), (BankPage::Dialog, 0));
        assert_eq!(page(CLASS_FIELD0), (BankPage::Field, 0));
        assert_eq!(page(0x26a), (BankPage::Field, 0));
        assert_eq!(page(CLASS_FIELD_LAST), (BankPage::Field, 0));
        assert_eq!(page(0), (BankPage::None, 0));
        assert_eq!(page(5), (BankPage::None, 0));
        assert_eq!(page(0x78), (BankPage::None, 0));
    }

    #[test]
    fn each_verb_page_class_yields_its_slot() {
        for slot in 0..BANK_SLOTS {
            assert_eq!(page(CLASS_SUB0 + slot), (BankPage::Sub, slot as u8));
        }
        assert_eq!(page(CLASS_SUB0 - 1), (BankPage::Field, 0));
        assert_eq!(page(CLASS_SUB0 + BANK_SLOTS), (BankPage::Edit, 0));
        assert_eq!(page(CLASS_SUB0 + BANK_SLOTS + 1), (BankPage::None, 0));
        assert_eq!(page(CLASS_FIELD_LAST + 1), (BankPage::Sub, 0));
    }

    fn item(token: u32) -> (ItemKind, u8, BankVerb, u8) {
        let item = decode_token(token);
        (item.kind, item.slot, item.verb, item.selection)
    }

    #[test]
    fn verb_tokens_name_a_slot_and_verb() {
        assert_eq!(item(0x40), (ItemKind::Verb, 0, BankVerb::SaveCurrent, 0));
        assert_eq!(item(0x41), (ItemKind::Verb, 0, BankVerb::EditName, 0));
        assert_eq!(item(0x42), (ItemKind::Verb, 0, BankVerb::Edit, 0));
        assert_eq!(item(0x43), (ItemKind::Verb, 0, BankVerb::Reset, 0));
        assert_eq!(item(0x44), (ItemKind::Verb, 1, BankVerb::SaveCurrent, 0));
        assert_eq!(item(0x58), (ItemKind::Verb, 6, BankVerb::SaveCurrent, 0));
        assert_eq!(item(0x5b), (ItemKind::Verb, 6, BankVerb::Reset, 0));
    }

    #[test]
    fn select_tokens_carry_the_selection_code() {
        assert_eq!(item(0x60), (ItemKind::Select, 0, BankVerb::SaveCurrent, 0));
        assert_eq!(item(0x61), (ItemKind::Select, 0, BankVerb::SaveCurrent, 1));
        assert_eq!(item(0x67), (ItemKind::Select, 0, BankVerb::SaveCurrent, 7));
    }

    #[test]
    fn token_gaps_are_not_actions() {
        for token in [0, 0x3f, 0x5c, 0x5f, 0x68, 0x70, 0x76, 0x7d, 0x7e, 0x80, 0x82, 0xffff_ffff] {
            assert_eq!(item(token), (ItemKind::None, 0, BankVerb::SaveCurrent, 0), "{token:#x}");
        }
        assert_eq!(item(0x60 + SELECT_ROWS as u32), (ItemKind::None, 0, BankVerb::SaveCurrent, 0));
        assert_eq!(
            item(TOKEN_VERB_BASE + BANK_SLOTS * VERBS_PER_SLOT),
            (ItemKind::None, 0, BankVerb::SaveCurrent, 0)
        );
    }

    #[test]
    fn c_abi_packs_the_classifiers() {
        assert_eq!(rb_ui_route_page(CLASS_SUB0 + 3), 2);
        assert_eq!(rb_ui_route_slot(CLASS_SUB0 + 3), 3);
        assert_eq!(rb_ui_route_page(CLASS_FIELD_LAST), 7);
        assert_eq!(rb_ui_route_slot(CLASS_FIELD_LAST), 0);
        assert_eq!(rb_ui_route_page(0x1234), 0);
        assert_eq!(rb_ui_route_item(0x4f), 1 | (3 << 8) | (3 << 16));
        assert_eq!(rb_ui_route_item(0x66), 2 | (6 << 24));
        assert_eq!(rb_ui_route_item(0x7d), 0);
    }
}
