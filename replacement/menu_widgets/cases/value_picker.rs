#![no_std]
#![no_main]
#[path = "../src/lib.rs"]
mod widgets;
pub use widgets::render_commands;
use widgets::render_commands::{DrawReport, FontId, NodeId, ResourceId, RowIndex};
use widgets::value_picker::*;

struct StockServices;
macro_rules! call {
    ($address:expr, $signature:ty $(, $argument:expr)*) => {{
        let function: $signature = unsafe { core::mem::transmute($address as usize) };
        unsafe { function($($argument),*) }
    }};
}
impl PickerServices for StockServices {
    fn empty_hook(&mut self) -> HookRecord {
        let mut record = core::mem::MaybeUninit::<HookRecord>::uninit();
        call!(0x43550, unsafe extern "C" fn(*mut HookRecord,u32,u32), record.as_mut_ptr(),0,64);
        unsafe { record.assume_init() }
    }
    fn clear_report(&mut self, report: &mut DrawReport) { call!(0x7221c8, unsafe extern "C" fn(*mut DrawReport,u32,u32), report,0,68); }
    fn panel_font(&mut self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x015e46c0 as *const u32) }) }
    fn snapshot(&mut self, node: NodeId, selection: SnapshotSelection) -> Snapshot {
        let mut value = Snapshot::default();
        match selection {
            SnapshotSelection::SavedSlotTwo => call!(0x61ce1c, unsafe extern "C" fn(u32,u32,*mut Snapshot), node.0,2,&mut value),
            SnapshotSelection::Live => call!(0x61cd14, unsafe extern "C" fn(*mut Snapshot), &mut value),
        }
        value
    }
    fn summary(&mut self, node: NodeId, parent: u32) -> Summary {
        let mut value = Summary::default();
        call!(0xbf8f4c, unsafe extern "C" fn(u32,u32,*mut Summary),node.0,parent,&mut value); value
    }
    fn child(&mut self, node: NodeId, parent: u32, index: RowIndex) -> Child {
        let mut value = Child::default();
        call!(0xbf90f0, unsafe extern "C" fn(u32,u32,u32,*mut Child),node.0,parent,index.0,&mut value); value
    }
    fn label(&mut self,node: NodeId,parent: u32,child: u32,field: LabelField) -> ResourceId {
        ResourceId(call!(0xbf93e0, unsafe extern "C" fn(u32,u32,u32,u32)->u32,node.0,parent,child,field as u32))
    }
    fn label_columns(&mut self,label: ResourceId,catalog: bool)->u32 {
        call!(if catalog {0x1112a6c} else {0x11391a0}, unsafe extern "C" fn(u32)->u32,label.0)
    }
    fn page_quotient(&mut self,numerator: u32,denominator: u32)->u32 { call!(0x4d978, unsafe extern "C" fn(u32,u32)->u32,numerator,denominator) }
    fn side_state(&mut self)->[u32;5] { unsafe { core::ptr::read_volatile(0x015e64d8 as *const [u32;5]) } }
    fn font(&mut self,font: FontId) { call!(0x1112b2c, unsafe extern "C" fn(u32),font.0); }
    fn palette(&mut self,p: Palette) { call!(0x1137cf8, unsafe extern "C" fn(u32,u32,u32,u32),p.index,p.alpha,p.component2,p.component3); }
    fn grid_palette(&mut self,palette: u32) { call!(0x1137d2c, unsafe extern "C" fn(u32),palette); }
    fn hook(&mut self,record: &mut HookRecord)->u32 { call!(0x116f948, unsafe extern "C" fn(*mut HookRecord)->u32,record) }
    fn resource(&mut self,record: &ResourceRecord) { call!(0x1112e78, unsafe extern "C" fn(*const ResourceRecord),record); }
    fn begin_format(&mut self) { call!(0x1112e38, unsafe extern "C" fn()); }
    fn end_format(&mut self) { call!(0x1112e58, unsafe extern "C" fn()); }
    fn palette_component(&mut self)->u32 { call!(0x1138a00, unsafe extern "C" fn(u32)->u32,2) }
    fn grid_resource(&mut self,x: i32,y: i32,r: ResourceId) { call!(0x111236c, unsafe extern "C" fn(i32,i32,u32),x,y,r.0); }
    fn grid_fill(&mut self,f: GridFill) { call!(0x1138060, unsafe extern "C" fn(i32,i32,i32,u32,u32,u32),f.left,f.right,f.row,f.height,f.mode,f.resource.0); }
    fn cell_fill(&mut self,f: CellFill) { call!(0x1113514, unsafe extern "C" fn(i32,i32,i32,i32,u32,u32,u32),f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn pixel_fill(&mut self,f: PixelFill) { call!(0x11130e0, unsafe extern "C" fn(i32,i32,u32,i32,u32,u32,u32),f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn text_format(&mut self,enabled: bool) { call!(0x1177e10, unsafe extern "C" fn(u32,u32,u32),0x01113bf4,enabled as u32,0); }
    fn bar_style(&mut self)->u32 { call!(0x1130e68, unsafe extern "C" fn(u32)->u32,0) }
    fn bar_resources(&mut self,style: u32)->[ResourceId;3] {
        let words = unsafe { core::ptr::read_volatile((0x0110ba2cu32.wrapping_add(style.wrapping_mul(12))) as *const [u32;3]) };
        words.map(ResourceId)
    }
    fn grid_size(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(0x111258c, unsafe extern "C" fn(u32,*mut u8),label.0,value.as_mut_ptr()); value
    }
    fn grid_extent(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(0x111299c, unsafe extern "C" fn(u32,u32,*mut u8),label.0,0,value.as_mut_ptr()); value
    }
    fn menu_mode(&mut self)->u32 { call!(0xbf96b0, unsafe extern "C" fn()->u32) }
    fn setting_help_hidden(&mut self)->bool { call!(0x61c93c, unsafe extern "C" fn(u32,u32)->u32,0x44,0) != 0 }
    fn metric_footer(&mut self,last: u8) { call!(0x1139a68, unsafe extern "C" fn(u32,u32,u32,u32,u32,u32,u32),5,2,last as u32,25,20,18,1); }
    fn resolution_footer(&mut self,node: NodeId,parent: u32,child: u32) { call!(0x1159168, unsafe extern "C" fn(u32,u32,u32),node.0,parent,child); }
}

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "C" fn rb_value_picker_case(node: u32,x: u32,y: u32,width: i32,column_step: u32,row_step: u32,style: u32,scrolling: u32,hide_help: u32,mode: u32,report: *mut DrawReport) {
    let args = DrawArgs { node: NodeId(node), x: x as u16, y: y as u16, width: width as i16,
        column_step,row_step,style,scrolling: scrolling != 0,hide_help: hide_help as u8,
        mode: DrawMode(mode), label_bound: if cfg!(picker_recipe_bank) { LabelBound::RecipeBank } else { LabelBound::Stock } };
    value_picker(&mut StockServices,args,unsafe { &mut *report });
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>)->! { loop { core::hint::spin_loop(); } }
