#![no_std]
#![no_main]
extern crate render_commands;
#[path = "../src/value_picker.rs"]
mod value_picker;
use render_commands::{DrawReport, FontId, NodeId, ResourceId, RowIndex};
use value_picker::*;

core::arch::global_asm!(r#"
.syntax unified
.arm
.macro RB_PICKER_CALL name, address
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
RB_PICKER_CALL rb_picker_call_43550, 0x43550
RB_PICKER_CALL rb_picker_call_7221c8, 0x7221c8
RB_PICKER_CALL rb_picker_call_61ce1c, 0x61ce1c
RB_PICKER_CALL rb_picker_call_61cd14, 0x61cd14
RB_PICKER_CALL rb_picker_call_bf8f4c, 0xbf8f4c
RB_PICKER_CALL rb_picker_call_bf90f0, 0xbf90f0
RB_PICKER_CALL rb_picker_call_bf93e0, 0xbf93e0
RB_PICKER_CALL rb_picker_call_1112a6c, 0x1112a6c
RB_PICKER_CALL rb_picker_call_11391a0, 0x11391a0
RB_PICKER_CALL rb_picker_call_4d978, 0x4d978
RB_PICKER_CALL rb_picker_call_1112b2c, 0x1112b2c
RB_PICKER_CALL rb_picker_call_1137cf8, 0x1137cf8
RB_PICKER_CALL rb_picker_call_1137d2c, 0x1137d2c
RB_PICKER_CALL rb_picker_call_116f948, 0x116f948
RB_PICKER_CALL rb_picker_call_1112e78, 0x1112e78
RB_PICKER_CALL rb_picker_call_1112e38, 0x1112e38
RB_PICKER_CALL rb_picker_call_1112e58, 0x1112e58
RB_PICKER_CALL rb_picker_call_1138a00, 0x1138a00
RB_PICKER_CALL rb_picker_call_111236c, 0x111236c
RB_PICKER_CALL rb_picker_call_1138060, 0x1138060
RB_PICKER_CALL rb_picker_call_1113514, 0x1113514
RB_PICKER_CALL rb_picker_call_11130e0, 0x11130e0
RB_PICKER_CALL rb_picker_call_1177e10, 0x1177e10
RB_PICKER_CALL rb_picker_call_1130e68, 0x1130e68
RB_PICKER_CALL rb_picker_call_111258c, 0x111258c
RB_PICKER_CALL rb_picker_call_111299c, 0x111299c
RB_PICKER_CALL rb_picker_call_bf96b0, 0xbf96b0
RB_PICKER_CALL rb_picker_call_61c93c, 0x61c93c
RB_PICKER_CALL rb_picker_call_1139a68, 0x1139a68
RB_PICKER_CALL rb_picker_call_1159168, 0x1159168
"#);

unsafe extern "C" {
    fn rb_picker_call_43550(record: *mut HookRecord, value: u32, bytes: u32);
    fn rb_picker_call_7221c8(report: *mut DrawReport, value: u32, bytes: u32);
    fn rb_picker_call_61ce1c(node: u32, slot: u32, out: *mut Snapshot);
    fn rb_picker_call_61cd14(out: *mut Snapshot);
    fn rb_picker_call_bf8f4c(node: u32, parent: u32, out: *mut Summary);
    fn rb_picker_call_bf90f0(node: u32, parent: u32, index: u32, out: *mut Child);
    fn rb_picker_call_bf93e0(node: u32, parent: u32, child: u32, field: u32) -> u32;
    fn rb_picker_call_1112a6c(label: u32) -> u32;
    fn rb_picker_call_11391a0(label: u32) -> u32;
    fn rb_picker_call_4d978(numerator: u32, denominator: u32) -> u32;
    fn rb_picker_call_1112b2c(font: u32);
    fn rb_picker_call_1137cf8(index: u32, alpha: u32, component2: u32, component3: u32);
    fn rb_picker_call_1137d2c(palette: u32);
    fn rb_picker_call_116f948(record: *mut HookRecord) -> u32;
    fn rb_picker_call_1112e78(record: *const ResourceRecord);
    fn rb_picker_call_1112e38();
    fn rb_picker_call_1112e58();
    fn rb_picker_call_1138a00(component: u32) -> u32;
    fn rb_picker_call_111236c(x: i32, y: i32, resource: u32);
    fn rb_picker_call_1138060(left: i32, right: i32, row: i32, height: u32, mode: u32, resource: u32);
    fn rb_picker_call_1113514(left: i32, top: i32, right: i32, bottom: i32, font: u32, palette: u32, alpha: u32);
    fn rb_picker_call_11130e0(left: i32, top: i32, right: u32, bottom: i32, font: u32, palette: u32, alpha: u32);
    fn rb_picker_call_1177e10(parser: u32, enabled: u32, reserved: u32);
    fn rb_picker_call_1130e68(index: u32) -> u32;
    fn rb_picker_call_111258c(label: u32, out: *mut u8);
    fn rb_picker_call_111299c(label: u32, reserved: u32, out: *mut u8);
    fn rb_picker_call_bf96b0() -> u32;
    fn rb_picker_call_61c93c(setting: u32, index: u32) -> u32;
    fn rb_picker_call_1139a68(first: u32, second: u32, last: u32, bottom: u32, width: u32, height: u32, mode: u32);
    fn rb_picker_call_1159168(node: u32, parent: u32, child: u32);
}

struct StockServices;
macro_rules! call {
    ($function:ident $(, $argument:expr)*) => {
        unsafe { $function($($argument),*) }
    };
}
impl PickerServices for StockServices {
    fn empty_hook(&mut self) -> HookRecord {
        let mut record = core::mem::MaybeUninit::<HookRecord>::uninit();
        call!(rb_picker_call_43550, record.as_mut_ptr(),0,64);
        unsafe { record.assume_init() }
    }
    fn clear_report(&mut self, report: &mut DrawReport) { call!(rb_picker_call_7221c8, report,0,68); }
    fn panel_font(&mut self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x015e46c0 as *const u32) }) }
    fn snapshot(&mut self, node: NodeId, selection: SnapshotSelection) -> Snapshot {
        let mut value = Snapshot::default();
        match selection {
            SnapshotSelection::SavedSlotTwo => call!(rb_picker_call_61ce1c, node.0,2,&mut value),
            SnapshotSelection::Live => call!(rb_picker_call_61cd14, &mut value),
        }
        value
    }
    fn summary(&mut self, node: NodeId, parent: u32) -> Summary {
        let mut value = Summary::default();
        call!(rb_picker_call_bf8f4c,node.0,parent,&mut value); value
    }
    fn child(&mut self, node: NodeId, parent: u32, index: RowIndex) -> Child {
        let mut value = Child::default();
        call!(rb_picker_call_bf90f0,node.0,parent,index.0,&mut value); value
    }
    fn label(&mut self,node: NodeId,parent: u32,child: u32,field: LabelField) -> ResourceId {
        ResourceId(call!(rb_picker_call_bf93e0,node.0,parent,child,field as u32))
    }
    fn label_columns(&mut self,label: ResourceId,catalog: bool)->u32 {
        if catalog { call!(rb_picker_call_1112a6c,label.0) } else { call!(rb_picker_call_11391a0,label.0) }
    }
    fn page_quotient(&mut self,numerator: u32,denominator: u32)->u32 { call!(rb_picker_call_4d978,numerator,denominator) }
    fn side_state(&mut self)->[u32;5] { unsafe { core::ptr::read_volatile(0x015e64d8 as *const [u32;5]) } }
    fn font(&mut self,font: FontId) { call!(rb_picker_call_1112b2c,font.0); }
    fn palette(&mut self,p: Palette) { call!(rb_picker_call_1137cf8,p.index,p.alpha,p.component2,p.component3); }
    fn grid_palette(&mut self,palette: u32) { call!(rb_picker_call_1137d2c,palette); }
    fn hook(&mut self,record: &mut HookRecord)->u32 { call!(rb_picker_call_116f948,record) }
    fn resource(&mut self,record: &ResourceRecord) { call!(rb_picker_call_1112e78,record); }
    fn begin_format(&mut self) { call!(rb_picker_call_1112e38); }
    fn end_format(&mut self) { call!(rb_picker_call_1112e58); }
    fn palette_component(&mut self)->u32 { call!(rb_picker_call_1138a00,2) }
    fn grid_resource(&mut self,x: i32,y: i32,r: ResourceId) { call!(rb_picker_call_111236c,x,y,r.0); }
    fn grid_fill(&mut self,f: GridFill) { call!(rb_picker_call_1138060,f.left,f.right,f.row,f.height,f.mode,f.resource.0); }
    fn cell_fill(&mut self,f: CellFill) { call!(rb_picker_call_1113514,f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn pixel_fill(&mut self,f: PixelFill) { call!(rb_picker_call_11130e0,f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn text_format(&mut self,enabled: bool) { call!(rb_picker_call_1177e10,0x01113bf4,enabled as u32,0); }
    fn bar_style(&mut self)->u32 { call!(rb_picker_call_1130e68,0) }
    fn bar_resources(&mut self,style: u32)->[ResourceId;3] {
        let words = unsafe { core::ptr::read_volatile((0x0110ba2cu32.wrapping_add(style.wrapping_mul(12))) as *const [u32;3]) };
        words.map(ResourceId)
    }
    fn grid_size(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(rb_picker_call_111258c,label.0,value.as_mut_ptr()); value
    }
    fn grid_extent(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(rb_picker_call_111299c,label.0,0,value.as_mut_ptr()); value
    }
    fn menu_mode(&mut self)->u32 { call!(rb_picker_call_bf96b0) }
    fn setting_help_hidden(&mut self)->bool { call!(rb_picker_call_61c93c,0x44,0) != 0 }
    fn metric_footer(&mut self,last: u8) { call!(rb_picker_call_1139a68,5,2,last as u32,25,20,18,1); }
    fn resolution_footer(&mut self,node: NodeId,parent: u32,child: u32) { call!(rb_picker_call_1159168,node.0,parent,child); }
}

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "C" fn rb_value_picker_case(node: u32,x: u32,y: u32,width: i32,column_step: u32,row_step: u32,style: u32,scrolling: u32,hide_help: u32,mode: u32,report: *mut DrawReport) {
    let args = DrawArgs { node: NodeId(node), x: x as u16, y: y as u16, width: width as i16,
        column_step,row_step,style,scrolling: scrolling != 0,hide_help: hide_help as u8,
        mode: DrawMode(mode), label_bound: if cfg!(picker_recipe_bank) { LabelBound::RecipeBank } else { LabelBound::Stock } };
    let _ = value_picker(&mut StockServices,args,unsafe { &mut *report });
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>)->! { loop { core::hint::spin_loop(); } }
