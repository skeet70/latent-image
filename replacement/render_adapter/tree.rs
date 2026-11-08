use crate::render_commands::{DrawReport, FontId, NodeId, ResourceId, SideState};
use crate::menu_widgets::tree_list::{Child, DrawServices, HookRecord, ResourceDescriptor, Snapshot, Summary};
use super::calls::*;

pub(super) struct StockServices;
unsafe extern "C" {
    fn rb_render_call_7221c8(a0: *mut DrawReport, a1: u32, a2: u32);
    fn rb_render_call_bf8e64(a0: u32, a1: *mut Summary);
    fn rb_render_call_4d978(a0: u32, a1: u32) -> u32;
    fn rb_render_call_bf9360(a0: u32, a1: u32, a2: u32) -> u32;
    fn rb_render_call_1112b2c(a0: u32);
    fn rb_render_call_1163e04(a0: u32, a1: u32);
    fn rb_render_call_1112da4();
    fn rb_render_call_1112d84();
    fn rb_render_call_1113514(a0: i32, a1: i32, a2: i32, a3: i32, a4: u32, a5: u32, a6: u32);
    fn rb_render_call_1177e10(a0: u32, a1: u32, a2: u32);
    fn rb_render_call_1163c1c(a0: u32, a1: u32);
    fn rb_render_call_116cf50(a0: u32, a1: u32) -> u32;
    fn rb_render_call_1112ce4(a0: u32);
    fn rb_render_call_bf93e0(a0: u32, a1: u32, a2: u32, a3: u32) -> u32;
    fn rb_render_call_1137cf8(a0: u32, a1: u32, a2: u32, a3: u32);
    fn rb_render_remainder(numerator: u32, denominator: u32) -> u32;
}
macro_rules! stock {
    ($name:ident, $($argument:expr),* $(,)?) => { unsafe { $name($($argument),*) } };
}
impl DrawServices for StockServices {
    fn initialize_hook(&mut self) -> HookRecord {
        let mut hook = core::mem::MaybeUninit::<HookRecord>::uninit();
        stock!(rb_render_call_43550, hook.as_mut_ptr().cast(), 0, 64);
        unsafe { hook.assume_init() }
    }
    fn panel_font(&self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x15e46c0 as *const u32) }) }
    fn live_snapshot(&mut self, out: &mut Snapshot) {
        stock!(rb_render_call_61cd14, (out as *mut Snapshot).cast());
    }
    fn clear_report(&mut self, out: &mut DrawReport) {
        stock!(rb_render_call_7221c8, out, 0, 68);
    }
    fn summarize(&mut self, node: NodeId, out: &mut Summary) {
        stock!(rb_render_call_bf8e64, node.0, out);
    }
    fn describe(&mut self, node: NodeId, index: u32, out: &mut Child) {
        stock!(rb_render_call_bf8d20, node.0, index, (out as *mut Child).cast());
    }
    fn divide(&mut self, numerator: u32, denominator: u32) -> u32 {
        stock!(rb_render_call_4d978, numerator, denominator)
    }
    fn remainder(&mut self, numerator: u32, denominator: u32) -> u32 {
        unsafe { rb_render_remainder(numerator, denominator) }
    }
    fn label(&mut self, node: NodeId, index: u32) -> ResourceId {
        ResourceId(stock!(rb_render_call_bf9360, node.0, index, 0))
    }
    fn font(&mut self, font: FontId) { stock!(rb_render_call_1112b2c, font.0); }
    fn side_state(&mut self, state: SideState) {
        unsafe {
            core::ptr::write_volatile(0x15e64e0 as *mut u32, state.alternate_font as u32);
            core::ptr::write_volatile(0x15e64d8 as *mut u32, state.kind);
        }
    }
    fn selection_palette(&mut self, selected: bool, state: u32) {
        stock!(rb_render_call_1163e04, selected as u32, state);
    }
    fn disable_format(&mut self) { stock!(rb_render_call_1112da4,); }
    fn enable_format(&mut self) { stock!(rb_render_call_1112d84,); }
    fn cell_box(&mut self, first:i32, second:i32, right:i32, bottom:i32, stroke:u32, colour:u32, alpha:u32) {
        stock!(rb_render_call_1113514, first,second,right,bottom,stroke,colour,alpha);
    }
    fn label_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(rb_render_call_116dce0, (record as *mut HookRecord).cast())
    }
    fn resource(&mut self, descriptor:&mut ResourceDescriptor) {
        stock!(rb_render_call_1112e78, (descriptor as *mut ResourceDescriptor).cast());
    }
    fn special_mode(&self) -> u32 { unsafe { core::ptr::read_volatile(0x15e64ec as *const u32) } }
    fn text_format(&mut self, enabled:bool) {
        stock!(rb_render_call_1177e10, 0x1113bf4, enabled as u32, 0);
    }
    fn descriptor_highlight(&mut self, state:u32) {
        stock!(rb_render_call_1163c1c, 0,state);
    }
    fn grid_resource(&mut self, first:u32, second:i32, resource:ResourceId) {
        stock!(rb_render_call_111236c, first as i32,second,resource.0);
    }
    fn palette_override(&mut self, node:NodeId, child:u32) -> u32 {
        stock!(rb_render_call_116cf50, node.0,child)
    }
    fn primary_palette(&mut self, palette:u32) { stock!(rb_render_call_1112ce4,palette); }
    fn value(&mut self, node:NodeId, child:u32, variant:u32) -> ResourceId {
        ResourceId(stock!(rb_render_call_bf93e0,node.0,child,variant,0))
    }
    fn value_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(rb_render_call_116e264, (record as *mut HookRecord).cast())
    }
    fn palette_components(&mut self,index:u32,first:u32,second:u32,third:u32) {
        stock!(rb_render_call_1137cf8,index,first,second,third);
    }
    fn grid_rectangle(&mut self,second:i32) {
        stock!(rb_render_call_1138060,24,30,second,1,2,0xff000101);
    }
}
