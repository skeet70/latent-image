#![no_std]

extern crate menu_engine;
extern crate menu_engine_adapter;
extern crate menu_widgets;
extern crate render_commands;

#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("entry.S"), options(raw));

pub(crate) mod class;
mod picker;
mod tree;
mod pane;
mod calls;

use class::{DrawFunction, EntryClass, ReturnAddress};
use crate::menu_widgets::value_picker::{value_picker, DrawArgs, DrawMode, LabelBound};
use crate::render_commands::{DrawReport, NodeId};
use core::ptr::{read_volatile, write_volatile};

static mut BUSY: bool = false;


unsafe extern "C" { fn rb_render_call_61ca2c() -> u32; }
#[repr(C)]
pub struct EntryRegisters { arguments: [u32; 4] }

fn classify(function: DrawFunction, registers: &EntryRegisters, stack: &[u32; 7], caller: u32) -> EntryClass {
    let node = unsafe { read_volatile(0x009571d0 as *const u32) };
    let origin = crate::menu_engine_adapter::render_node_origin(
        crate::menu_engine::frame::NodeId::new(registers.arguments[0]),
        crate::menu_engine::block::RecordAddress::new(node),
    );
    let preliminary = EntryClass {
        function, caller: ReturnAddress(caller), mode: stack[5], node: registers.arguments[0],
        live_kind: 1, live_parent: 0, live_selected: 0, origin,
    };
    let (live_kind, live_parent, live_selected) = if function == DrawFunction::Tree && preliminary.owned() && node != 0 {
        let parent = unsafe { read_volatile((node + 8) as *const u32) };
        let rows = unsafe { read_volatile((node + 0x20) as *const u32) };
        let cursor = unsafe { read_volatile(0x009571d8 as *const u32) };
        let selected = if rows == 0 { 0 } else { unsafe { read_volatile((rows + cursor * 8) as *const u32) } };
        (unsafe { rb_render_call_61ca2c() }, parent, selected)
    } else { (1, 0, 0) };
    EntryClass {
        function, caller: ReturnAddress(caller), mode: stack[5], node: registers.arguments[0],
        live_kind, live_parent, live_selected, origin,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rb_render_picker_apply(registers: &EntryRegisters, stack: &[u32; 7], caller: u32, report: *mut DrawReport) -> u32 {
    if unsafe { read_volatile(&raw const BUSY) } { return 0; }
    unsafe { write_volatile(&raw mut BUSY, true); }
    let class = classify(DrawFunction::Picker, registers, stack, caller);
    if !class.owned() {
        unsafe { write_volatile(&raw mut BUSY, false); }
        return 0;
    }
    let [node, x, y, width] = registers.arguments;
    let args = DrawArgs {
        node: NodeId(node), x: x as u16, y: y as u16, width: width as i16,
        column_step: stack[0], row_step: stack[1], style: stack[2], scrolling: stack[3] != 0,
        hide_help: stack[4] as u8, mode: DrawMode(stack[5]), label_bound: LabelBound::RecipeBank,
    };
    value_picker(&mut picker::StockServices, args, unsafe { &mut *report });
    unsafe { write_volatile(&raw mut BUSY, false); }
    1
}

#[no_mangle]
pub unsafe extern "C" fn rb_render_tree_apply(registers: &EntryRegisters, stack: &[u32; 7], caller: u32, report: *mut DrawReport) -> u32 {
    if unsafe { read_volatile(&raw const BUSY) } { return 0; }
    unsafe { write_volatile(&raw mut BUSY, true); }
    let class = classify(DrawFunction::Tree, registers, stack, caller);
    if !class.owned() {
        unsafe { write_volatile(&raw mut BUSY, false); }
        return 0;
    }
    let [node, first, second, first_step] = registers.arguments;
    let args = crate::menu_widgets::tree_list::DrawArgs {
        node: NodeId(node), first: first as u16, second: second as u16, first_step,
        nested: crate::menu_widgets::tree_list::NestedGrid {
            first: stack[1] as u16, second: stack[2] as u16,
            first_step: stack[3], second_step: stack[4],
        },
        second_step: stack[0], mode: crate::menu_widgets::tree_list::DrawMode(stack[5]),
    };
    if crate::menu_widgets::tree_list::tree_list(&mut tree::StockServices, args, unsafe { &mut *report }).is_err() {
        unsafe { write_volatile(&raw mut BUSY, false); }
        return 0;
    }
    unsafe { write_volatile(&raw mut BUSY, false); }
    1
}

#[no_mangle]
pub unsafe extern "C" fn rb_render_pane_apply(registers: &EntryRegisters, _stack: *const u32, caller: u32, _report: *mut DrawReport) -> u32 {
    if unsafe { read_volatile(&raw const BUSY) } { return 0; }
    unsafe { write_volatile(&raw mut BUSY, true); }
    let [node, mode, _, _] = registers.arguments;
    let record = unsafe { read_volatile(0x009571d0 as *const u32) };
    let origin = crate::menu_engine_adapter::render_node_origin(
        crate::menu_engine::frame::NodeId::new(node),
        crate::menu_engine::block::RecordAddress::new(record),
    );
    let depth = if origin == class::NodeOrigin::Stock { unsafe { rb_render_call_61ca2c() } } else { u32::MAX };
    let kind = if origin == class::NodeOrigin::Stock { unsafe { read_volatile((record + 4) as *const u32) } } else { u32::MAX };
    let class = class::PaneEntryClass {
        caller: ReturnAddress(caller), node: class::PaneNode(node), mode: class::PaneMode(mode),
        depth: class::SnapshotDepth(depth), kind: class::SnapshotKind(kind), origin,
    };
    let mut owned = class.owned();
    if owned {
        owned = crate::menu_widgets::pane_drawer::pane_drawer(&mut pane::StockServices, NodeId(node), crate::menu_widgets::pane_drawer::DrawMode(mode)).is_ok();
    }
    unsafe { write_volatile(&raw mut BUSY, false); }
    owned as u32
}
