#![no_std]
#![no_main]

#[path = "../../render_commands/src/lib.rs"]
mod render_commands;
#[path = "../src/tree_list.rs"]
mod tree_list;

use render_commands::{DrawReport, FontId, NodeId, ResourceId, SideState};
use tree_list::{Child, DrawArgs, DrawMode, DrawServices, HookRecord, NestedChild, NestedGrid, ResourceDescriptor, Selection, Snapshot, SnapshotSource, Summary};

core::arch::global_asm!(r#"
.syntax unified
.arm
.macro RB_TREE_CALL name, address
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    movw r12, #:lower16:\address
    movt r12, #:upper16:\address
    bx r12
.size \name, .-\name
.endm
RB_TREE_CALL rb_tree_call_43550, 0x43550
RB_TREE_CALL rb_tree_call_61cd14, 0x61cd14
RB_TREE_CALL rb_tree_call_61ce1c, 0x61ce1c
RB_TREE_CALL rb_tree_call_11730dc, 0x11730dc
RB_TREE_CALL rb_tree_call_bf8f4c, 0xbf8f4c
RB_TREE_CALL rb_tree_call_bf90f0, 0xbf90f0
RB_TREE_CALL rb_tree_call_bf93e0, 0xbf93e0
RB_TREE_CALL rb_tree_call_116f948, 0x116f948
RB_TREE_CALL rb_tree_call_116d4e4, 0x116d4e4
RB_TREE_CALL rb_tree_call_1159168, 0x1159168
RB_TREE_CALL rb_tree_call_7221c8, 0x7221c8
RB_TREE_CALL rb_tree_call_bf8e64, 0xbf8e64
RB_TREE_CALL rb_tree_call_bf8d20, 0xbf8d20
RB_TREE_CALL rb_tree_call_4d978, 0x4d978
RB_TREE_CALL rb_tree_call_bf9360, 0xbf9360
RB_TREE_CALL rb_tree_call_1112b2c, 0x1112b2c
RB_TREE_CALL rb_tree_call_1163e04, 0x1163e04
RB_TREE_CALL rb_tree_call_1112da4, 0x1112da4
RB_TREE_CALL rb_tree_call_1112d84, 0x1112d84
RB_TREE_CALL rb_tree_call_1113514, 0x1113514
RB_TREE_CALL rb_tree_call_116dce0, 0x116dce0
RB_TREE_CALL rb_tree_call_1112e78, 0x1112e78
RB_TREE_CALL rb_tree_call_1177e10, 0x1177e10
RB_TREE_CALL rb_tree_call_1163c1c, 0x1163c1c
RB_TREE_CALL rb_tree_call_111236c, 0x111236c
RB_TREE_CALL rb_tree_call_116cf50, 0x116cf50
RB_TREE_CALL rb_tree_call_1112ce4, 0x1112ce4
RB_TREE_CALL rb_tree_call_116e264, 0x116e264
RB_TREE_CALL rb_tree_call_1137cf8, 0x1137cf8
RB_TREE_CALL rb_tree_call_1138060, 0x1138060
"#);

unsafe extern "C" {
    fn rb_tree_call_43550(record: *mut HookRecord, value: u32, bytes: u32);
    fn rb_tree_call_61cd14(out: *mut Snapshot);
    fn rb_tree_call_61ce1c(node: u32, slot: u32, out: *mut Snapshot);
    fn rb_tree_call_11730dc(node: u32, kind: u32, record: *mut u32, selection: *mut u32, index: *mut u32) -> u32;
    fn rb_tree_call_bf8f4c(node: u32, parent: u32, out: *mut Summary);
    fn rb_tree_call_bf90f0(node: u32, parent: u32, index: u32, out: *mut NestedChild);
    fn rb_tree_call_bf93e0(node: u32, parent: u32, selection: u32, field: u32) -> u32;
    fn rb_tree_call_116f948(record: *mut HookRecord) -> u32;
    fn rb_tree_call_116d4e4(node: u32, parent: u32, selection: u32);
    fn rb_tree_call_1159168(node: u32, parent: u32, selection: u32);
    fn rb_tree_call_7221c8(report: *mut DrawReport, value: u32, bytes: u32);
    fn rb_tree_call_bf8e64(node: u32, out: *mut Summary);
    fn rb_tree_call_bf8d20(node: u32, index: u32, out: *mut Child);
    fn rb_tree_call_4d978(numerator: u32, denominator: u32) -> u32;
    fn rb_tree_call_bf9360(node: u32, index: u32, field: u32) -> u32;
    fn rb_tree_call_1112b2c(font: u32);
    fn rb_tree_call_1163e04(selected: u32, state: u32);
    fn rb_tree_call_1112da4();
    fn rb_tree_call_1112d84();
    fn rb_tree_call_1113514(first: i32, second: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32);
    fn rb_tree_call_116dce0(record: *mut HookRecord) -> u32;
    fn rb_tree_call_1112e78(descriptor: *mut ResourceDescriptor);
    fn rb_tree_call_1177e10(parser: u32, enabled: u32, reserved: u32);
    fn rb_tree_call_1163c1c(selected: u32, state: u32);
    fn rb_tree_call_111236c(first: u32, second: i32, resource: u32);
    fn rb_tree_call_116cf50(node: u32, child: u32) -> u32;
    fn rb_tree_call_1112ce4(palette: u32);
    fn rb_tree_call_116e264(record: *mut HookRecord) -> u32;
    fn rb_tree_call_1137cf8(index: u32, first: u32, second: u32, third: u32);
    fn rb_tree_call_1138060(left: u32, right: u32, row: i32, height: u32, mode: u32, resource: u32);
}

macro_rules! stock {
    ($function:ident, $($argument:expr),* $(,)?) => { unsafe { $function($($argument),*) } };
}

struct StockServices;
impl DrawServices for StockServices {
    fn initialize_hook(&mut self) -> HookRecord {
        let mut hook = core::mem::MaybeUninit::<HookRecord>::uninit();
        stock!(rb_tree_call_43550, hook.as_mut_ptr(), 0, 64);
        unsafe { hook.assume_init() }
    }
    fn panel_font(&self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x15e46c0 as *const u32) }) }
    fn snapshot(&mut self, node: NodeId, source: SnapshotSource, out: &mut Snapshot) {
        match source {
            SnapshotSource::Live => stock!(rb_tree_call_61cd14, out),
            SnapshotSource::SavedSlot1 => stock!(rb_tree_call_61ce1c, node.0,1,out),
            SnapshotSource::SavedSlot2 => stock!(rb_tree_call_61ce1c, node.0,2,out),
        }
    }
    fn resolve_selection(&mut self, node: NodeId, selection: &mut Selection) -> bool {
        stock!(rb_tree_call_11730dc, node.0,2,&mut selection.record,&mut selection.selection,&mut selection.index) != 0
    }
    fn nested_summary(&mut self, node: NodeId, parent: u32, out: &mut Summary) {
        stock!(rb_tree_call_bf8f4c, node.0,parent,out);
    }
    fn nested_child(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild) {
        stock!(rb_tree_call_bf90f0, node.0,parent,index,out);
    }
    fn nested_label(&mut self, node: NodeId, parent: u32, selection: u32) -> ResourceId {
        ResourceId(stock!(rb_tree_call_bf93e0,node.0,parent,selection,1))
    }
    fn selection_hook(&mut self, record: &mut HookRecord) -> u32 {
        stock!(rb_tree_call_116f948,record)
    }
    fn selected_detail(&mut self, node: NodeId, parent: u32, selection: u32) {
        stock!(rb_tree_call_116d4e4,node.0,parent,selection);
    }
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32) {
        stock!(rb_tree_call_1159168,node.0,parent,selection);
    }
    fn clear_report(&mut self, out: &mut DrawReport) {
        stock!(rb_tree_call_7221c8, out, 0, 68);
    }
    fn summarize(&mut self, node: NodeId, out: &mut Summary) {
        stock!(rb_tree_call_bf8e64, node.0, out);
    }
    fn describe(&mut self, node: NodeId, index: u32, out: &mut Child) {
        stock!(rb_tree_call_bf8d20, node.0, index, out);
    }
    fn divide(&mut self, numerator: u32, denominator: u32) -> u32 {
        stock!(rb_tree_call_4d978, numerator, denominator)
    }
    fn remainder(&mut self, numerator: u32, denominator: u32) -> u32 {
        let remainder;
        unsafe {
            core::arch::asm!("movw r12, #0xda6c", "movt r12, #4", "blx r12", inout("r0") numerator => _, inout("r1") denominator => remainder, out("r2") _, out("r3") _, out("r12") _, out("lr") _);
        }
        remainder
    }
    fn label(&mut self, node: NodeId, index: u32) -> ResourceId {
        ResourceId(stock!(rb_tree_call_bf9360, node.0, index, 0))
    }
    fn font(&mut self, font: FontId) { stock!(rb_tree_call_1112b2c, font.0); }
    fn side_state(&mut self, state: SideState) {
        unsafe {
            core::ptr::write_volatile(0x15e64e0 as *mut u32, state.alternate_font as u32);
            core::ptr::write_volatile(0x15e64d8 as *mut u32, state.kind);
        }
    }
    fn selection_palette(&mut self, selected: bool, state: u32) {
        stock!(rb_tree_call_1163e04, selected as u32, state);
    }
    fn disable_format(&mut self) { stock!(rb_tree_call_1112da4,); }
    fn enable_format(&mut self) { stock!(rb_tree_call_1112d84,); }
    fn cell_box(&mut self, first:i32, second:i32, right:i32, bottom:i32, stroke:u32, colour:u32, alpha:u32) {
        stock!(rb_tree_call_1113514, first,second,right,bottom,stroke,colour,alpha);
    }
    fn label_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(rb_tree_call_116dce0, record)
    }
    fn resource(&mut self, descriptor:&mut ResourceDescriptor) {
        stock!(rb_tree_call_1112e78, descriptor);
    }
    fn special_mode(&self) -> u32 { unsafe { core::ptr::read_volatile(0x15e64ec as *const u32) } }
    fn text_format(&mut self, enabled:bool) {
        stock!(rb_tree_call_1177e10, 0x1113bf4, enabled as u32, 0);
    }
    fn descriptor_highlight(&mut self, state:u32) {
        stock!(rb_tree_call_1163c1c, 0,state);
    }
    fn grid_resource(&mut self, first:u32, second:i32, resource:ResourceId) {
        stock!(rb_tree_call_111236c, first,second,resource.0);
    }
    fn palette_override(&mut self, node:NodeId, child:u32) -> u32 {
        stock!(rb_tree_call_116cf50, node.0,child)
    }
    fn primary_palette(&mut self, palette:u32) { stock!(rb_tree_call_1112ce4,palette); }
    fn value(&mut self, node:NodeId, child:u32, variant:u32) -> ResourceId {
        ResourceId(stock!(rb_tree_call_bf93e0,node.0,child,variant,0))
    }
    fn value_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(rb_tree_call_116e264,record)
    }
    fn palette_components(&mut self,index:u32,first:u32,second:u32,third:u32) {
        stock!(rb_tree_call_1137cf8,index,first,second,third);
    }
    fn grid_rectangle(&mut self,second:i32) {
        stock!(rb_tree_call_1138060,24,30,second,1,2,0xff000101);
    }
}

#[no_mangle]
pub unsafe extern "C" fn tree_list_entry(node:u32,first:u32,second:u32,first_step:u32,second_step:u32,nested_first:u32,nested_second:u32,nested_first_step:u32,nested_second_step:u32,mode:u32,report:*mut DrawReport) {
    tree_list::tree_list(&mut StockServices,DrawArgs { node:NodeId(node),first:first as u16,second:second as u16,first_step,second_step,nested:NestedGrid {first:nested_first as u16,second:nested_second as u16,first_step:nested_first_step,second_step:nested_second_step},mode:DrawMode(mode) }, unsafe { &mut *report });
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop { core::hint::spin_loop(); } }
