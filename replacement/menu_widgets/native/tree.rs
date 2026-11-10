#![no_std]
#![no_main]

#[path = "../../render_commands/src/lib.rs"]
mod render_commands;
#[path = "../src/tree_list.rs"]
mod tree_list;

use render_commands::{DrawReport, FontId, NodeId, ResourceId, SideState};
use tree_list::{Child, DrawArgs, DrawMode, DrawServices, HookRecord, NestedChild, NestedGrid, ResourceDescriptor, Selection, Snapshot, SnapshotSource, Summary};

macro_rules! stock_target {
    ($address:expr, $result:ty, [$($name:ident: $argument_type:ty),*], [$($argument:expr),*]) => {{
        #[inline(never)]
        unsafe extern "C" fn invoke<const ADDRESS: usize>($($name: $argument_type),*) -> $result {
            let function: unsafe extern "C" fn($($argument_type),*) -> $result =
                unsafe { core::mem::transmute(ADDRESS) };
            unsafe { function($($name),*) }
        }
        unsafe { invoke::<{ $address }>($($argument),*) }
    }};
}

macro_rules! stock {
    ($address:expr, unsafe extern "C" fn($($argument_type:ty),*), $($argument:expr),* $(,)?) => {
        stock!($address, unsafe extern "C" fn($($argument_type),*) -> (), $($argument),*)
    };
    ($address:expr, unsafe extern "C" fn() -> $result:ty, $(,)?) => {
        stock_target!($address, $result, [], [])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty, $t2:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1, a2: $t2], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty, $t2:ty, $t3:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1, a2: $t2, a3: $t3], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty, $t2:ty, $t3:ty, $t4:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1, a2: $t2, a3: $t3, a4: $t4], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty, $t2:ty, $t3:ty, $t4:ty, $t5:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1, a2: $t2, a3: $t3, a4: $t4, a5: $t5], [$($argument),*])
    };
    ($address:expr, unsafe extern "C" fn($t0:ty, $t1:ty, $t2:ty, $t3:ty, $t4:ty, $t5:ty, $t6:ty) -> $result:ty, $($argument:expr),* $(,)?) => {
        stock_target!($address, $result, [a0: $t0, a1: $t1, a2: $t2, a3: $t3, a4: $t4, a5: $t5, a6: $t6], [$($argument),*])
    };
}

struct StockServices;
impl DrawServices for StockServices {
    fn initialize_hook(&mut self) -> HookRecord {
        let mut hook = core::mem::MaybeUninit::<HookRecord>::uninit();
        stock!(0x43550, unsafe extern "C" fn(*mut HookRecord, u32, u32), hook.as_mut_ptr(), 0, 64);
        unsafe { hook.assume_init() }
    }
    fn panel_font(&self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x15e46c0 as *const u32) }) }
    fn snapshot(&mut self, node: NodeId, source: SnapshotSource, out: &mut Snapshot) {
        match source {
            SnapshotSource::Live => stock!(0x61cd14, unsafe extern "C" fn(*mut Snapshot), out),
            SnapshotSource::SavedSlot1 => stock!(0x61ce1c, unsafe extern "C" fn(u32,u32,*mut Snapshot), node.0,1,out),
            SnapshotSource::SavedSlot2 => stock!(0x61ce1c, unsafe extern "C" fn(u32,u32,*mut Snapshot), node.0,2,out),
        }
    }
    fn resolve_selection(&mut self, node: NodeId, selection: &mut Selection) -> bool {
        stock!(0x11730dc, unsafe extern "C" fn(u32,u32,*mut u32,*mut u32,*mut u32)->u32, node.0,2,&mut selection.record,&mut selection.selection,&mut selection.index) != 0
    }
    fn nested_summary(&mut self, node: NodeId, parent: u32, out: &mut Summary) {
        stock!(0xbf8f4c, unsafe extern "C" fn(u32,u32,*mut Summary), node.0,parent,out);
    }
    fn nested_child(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild) {
        stock!(0xbf90f0, unsafe extern "C" fn(u32,u32,u32,*mut NestedChild), node.0,parent,index,out);
    }
    fn nested_label(&mut self, node: NodeId, parent: u32, selection: u32) -> ResourceId {
        ResourceId(stock!(0xbf93e0, unsafe extern "C" fn(u32,u32,u32,u32)->u32,node.0,parent,selection,1))
    }
    fn selection_hook(&mut self, record: &mut HookRecord) -> u32 {
        stock!(0x116f948, unsafe extern "C" fn(*mut HookRecord)->u32,record)
    }
    fn selected_detail(&mut self, node: NodeId, parent: u32, selection: u32) {
        stock!(0x116d4e4, unsafe extern "C" fn(u32,u32,u32),node.0,parent,selection);
    }
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32) {
        stock!(0x1159168, unsafe extern "C" fn(u32,u32,u32),node.0,parent,selection);
    }
    fn clear_report(&mut self, out: &mut DrawReport) {
        stock!(0x7221c8, unsafe extern "C" fn(*mut DrawReport, u32, u32), out, 0, 68);
    }
    fn summarize(&mut self, node: NodeId, out: &mut Summary) {
        stock!(0xbf8e64, unsafe extern "C" fn(u32, *mut Summary), node.0, out);
    }
    fn describe(&mut self, node: NodeId, index: u32, out: &mut Child) {
        stock!(0xbf8d20, unsafe extern "C" fn(u32, u32, *mut Child), node.0, index, out);
    }
    fn divide(&mut self, numerator: u32, denominator: u32) -> u32 {
        stock!(0x4d978, unsafe extern "C" fn(u32, u32) -> u32, numerator, denominator)
    }
    fn remainder(&mut self, numerator: u32, denominator: u32) -> u32 {
        let remainder;
        unsafe {
            core::arch::asm!("movw r12, #0xda6c", "movt r12, #4", "blx r12", inout("r0") numerator => _, inout("r1") denominator => remainder, out("r2") _, out("r3") _, out("r12") _, out("lr") _);
        }
        remainder
    }
    fn label(&mut self, node: NodeId, index: u32) -> ResourceId {
        ResourceId(stock!(0xbf9360, unsafe extern "C" fn(u32, u32, u32) -> u32, node.0, index, 0))
    }
    fn font(&mut self, font: FontId) { stock!(0x1112b2c, unsafe extern "C" fn(u32), font.0); }
    fn side_state(&mut self, state: SideState) {
        unsafe {
            core::ptr::write_volatile(0x15e64e0 as *mut u32, state.alternate_font as u32);
            core::ptr::write_volatile(0x15e64d8 as *mut u32, state.kind);
        }
    }
    fn selection_palette(&mut self, selected: bool, state: u32) {
        stock!(0x1163e04, unsafe extern "C" fn(u32,u32), selected as u32, state);
    }
    fn disable_format(&mut self) { stock!(0x1112da4, unsafe extern "C" fn(),); }
    fn enable_format(&mut self) { stock!(0x1112d84, unsafe extern "C" fn(),); }
    fn cell_box(&mut self, first:i32, second:i32, right:i32, bottom:i32, stroke:u32, colour:u32, alpha:u32) {
        stock!(0x1113514, unsafe extern "C" fn(i32,i32,i32,i32,u32,u32,u32), first,second,right,bottom,stroke,colour,alpha);
    }
    fn label_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(0x116dce0, unsafe extern "C" fn(*mut HookRecord)->u32, record)
    }
    fn resource(&mut self, descriptor:&mut ResourceDescriptor) {
        stock!(0x1112e78, unsafe extern "C" fn(*mut ResourceDescriptor), descriptor);
    }
    fn special_mode(&self) -> u32 { unsafe { core::ptr::read_volatile(0x15e64ec as *const u32) } }
    fn text_format(&mut self, enabled:bool) {
        stock!(0x1177e10, unsafe extern "C" fn(u32,u32,u32), 0x1113bf4, enabled as u32, 0);
    }
    fn descriptor_highlight(&mut self, state:u32) {
        stock!(0x1163c1c, unsafe extern "C" fn(u32,u32), 0,state);
    }
    fn grid_resource(&mut self, first:u32, second:i32, resource:ResourceId) {
        stock!(0x111236c, unsafe extern "C" fn(u32,i32,u32), first,second,resource.0);
    }
    fn palette_override(&mut self, node:NodeId, child:u32) -> u32 {
        stock!(0x116cf50, unsafe extern "C" fn(u32,u32)->u32, node.0,child)
    }
    fn primary_palette(&mut self, palette:u32) { stock!(0x1112ce4, unsafe extern "C" fn(u32),palette); }
    fn value(&mut self, node:NodeId, child:u32, variant:u32) -> ResourceId {
        ResourceId(stock!(0xbf93e0, unsafe extern "C" fn(u32,u32,u32,u32)->u32,node.0,child,variant,0))
    }
    fn value_hook(&mut self, record:&mut HookRecord) -> u32 {
        stock!(0x116e264, unsafe extern "C" fn(*mut HookRecord)->u32,record)
    }
    fn palette_components(&mut self,index:u32,first:u32,second:u32,third:u32) {
        stock!(0x1137cf8, unsafe extern "C" fn(u32,u32,u32,u32),index,first,second,third);
    }
    fn grid_rectangle(&mut self,second:i32) {
        stock!(0x1138060, unsafe extern "C" fn(u32,u32,i32,u32,u32,u32),24,30,second,1,2,0xff000101);
    }
}

#[no_mangle]
pub unsafe extern "C" fn tree_list_entry(node:u32,first:u32,second:u32,first_step:u32,second_step:u32,nested_first:u32,nested_second:u32,nested_first_step:u32,nested_second_step:u32,mode:u32,report:*mut DrawReport) {
    tree_list::tree_list(&mut StockServices,DrawArgs { node:NodeId(node),first:first as u16,second:second as u16,first_step,second_step,nested:NestedGrid {first:nested_first as u16,second:nested_second as u16,first_step:nested_first_step,second_step:nested_second_step},mode:DrawMode(mode) }, unsafe { &mut *report });
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop { core::hint::spin_loop(); } }
