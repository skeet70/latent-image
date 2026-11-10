use crate::render_commands::{DrawReport, FontId, NodeId, ResourceId, RowIndex};
use crate::menu_widgets::value_picker::*;
use super::calls::*;

pub(super) struct StockServices;
unsafe extern "C" {
    fn rb_render_call_7221c8(a0: *mut DrawReport, a1: u32, a2: u32);
    fn rb_render_call_61ce1c(a0: u32, a1: u32, a2: *mut Snapshot);
    fn rb_render_call_bf8f4c(a0: u32, a1: u32, a2: *mut Summary);
    fn rb_render_call_bf93e0(a0: u32, a1: u32, a2: u32, a3: u32) -> u32;
    fn rb_render_call_1112a6c(a0: u32) -> u32;
    fn rb_render_call_11391a0(a0: u32) -> u32;
    fn rb_render_call_4d978(a0: u32, a1: u32) -> u32;
    fn rb_render_call_1112b2c(a0: u32);
    fn rb_render_call_1137cf8(a0: u32, a1: u32, a2: u32, a3: u32);
    fn rb_render_call_1137d2c(a0: u32);
    fn rb_render_call_1112e38();
    fn rb_render_call_1112e58();
    fn rb_render_call_1138a00(a0: u32) -> u32;
    fn rb_render_call_1113514(a0: i32, a1: i32, a2: i32, a3: i32, a4: u32, a5: u32, a6: u32);
    fn rb_render_call_11130e0(a0: i32, a1: i32, a2: u32, a3: i32, a4: u32, a5: u32, a6: u32);
    fn rb_render_call_1177e10(a0: u32, a1: u32, a2: u32);
    fn rb_render_call_1130e68(a0: u32) -> u32;
    fn rb_render_call_111258c(a0: u32, a1: *mut u8);
    fn rb_render_call_111299c(a0: u32, a1: u32, a2: *mut u8);
    fn rb_render_call_bf96b0() -> u32;
    fn rb_render_call_61c93c(a0: u32, a1: u32) -> u32;
    fn rb_render_call_1139a68(a0: u32, a1: u32, a2: u32, a3: u32, a4: u32, a5: u32, a6: u32);
    fn rb_render_call_1159168(a0: u32, a1: u32, a2: u32);
}
macro_rules! call {
    ($name:ident $(, $argument:expr)*) => { unsafe { $name($($argument),*) } };
}
impl PickerServices for StockServices {
    fn empty_hook(&mut self) -> HookRecord {
        let mut record = core::mem::MaybeUninit::<HookRecord>::uninit();
        call!(rb_render_call_43550, record.as_mut_ptr().cast(),0,64);
        unsafe { record.assume_init() }
    }
    fn clear_report(&mut self, report: &mut DrawReport) { call!(rb_render_call_7221c8, report,0,68); }
    fn panel_font(&mut self) -> FontId { FontId(unsafe { core::ptr::read_volatile(0x015e46c0 as *const u32) }) }
    fn snapshot(&mut self, node: NodeId, selection: SnapshotSelection) -> Snapshot {
        let mut value = Snapshot::default();
        match selection {
            SnapshotSelection::SavedSlotTwo => call!(rb_render_call_61ce1c, node.0,2,&mut value),
            SnapshotSelection::Live => call!(rb_render_call_61cd14, (&mut value as *mut Snapshot).cast()),
        }
        value
    }
    fn summary(&mut self, node: NodeId, parent: u32) -> Summary {
        let mut value = Summary::default();
        call!(rb_render_call_bf8f4c,node.0,parent,&mut value); value
    }
    fn child(&mut self, node: NodeId, parent: u32, index: RowIndex) -> Child {
        let mut value = Child::default();
        call!(rb_render_call_bf90f0,node.0,parent,index.0,(&mut value as *mut Child).cast()); value
    }
    fn label(&mut self,node: NodeId,parent: u32,child: u32,field: LabelField) -> ResourceId {
        ResourceId(call!(rb_render_call_bf93e0,node.0,parent,child,field as u32))
    }
    fn label_columns(&mut self,label: ResourceId,catalog: bool)->u32 {
        if catalog { call!(rb_render_call_1112a6c,label.0) } else { call!(rb_render_call_11391a0,label.0) }
    }
    fn page_quotient(&mut self,numerator: u32,denominator: u32)->u32 { call!(rb_render_call_4d978,numerator,denominator) }
    fn side_state(&mut self)->[u32;5] { unsafe { core::ptr::read_volatile(0x015e64d8 as *const [u32;5]) } }
    fn font(&mut self,font: FontId) { call!(rb_render_call_1112b2c,font.0); }
    fn palette(&mut self,p: Palette) { call!(rb_render_call_1137cf8,p.index,p.alpha,p.component2,p.component3); }
    fn grid_palette(&mut self,palette: u32) { call!(rb_render_call_1137d2c,palette); }
    fn hook(&mut self,record: &mut HookRecord)->u32 { call!(rb_render_call_116f948,(record as *mut HookRecord).cast()) }
    fn resource(&mut self,record: &ResourceRecord) { call!(rb_render_call_1112e78,(record as *const ResourceRecord).cast()); }
    fn begin_format(&mut self) { call!(rb_render_call_1112e38); }
    fn end_format(&mut self) { call!(rb_render_call_1112e58); }
    fn palette_component(&mut self)->u32 { call!(rb_render_call_1138a00,2) }
    fn grid_resource(&mut self,x: i32,y: i32,r: ResourceId) { call!(rb_render_call_111236c,x,y,r.0); }
    fn grid_fill(&mut self,f: GridFill) { call!(rb_render_call_1138060,f.left,f.right,f.row,f.height,f.mode,f.resource.0); }
    fn cell_fill(&mut self,f: CellFill) { call!(rb_render_call_1113514,f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn pixel_fill(&mut self,f: PixelFill) { call!(rb_render_call_11130e0,f.left,f.top,f.right,f.bottom,f.font.0,f.palette,f.alpha); }
    fn text_format(&mut self,enabled: bool) { call!(rb_render_call_1177e10,0x01113bf4,enabled as u32,0); }
    fn bar_style(&mut self)->u32 { call!(rb_render_call_1130e68,0) }
    fn bar_resources(&mut self,style: u32)->[ResourceId;3] {
        let words = unsafe { core::ptr::read_volatile((0x0110ba2cu32.wrapping_add(style.wrapping_mul(12))) as *const [u32;3]) };
        words.map(ResourceId)
    }
    fn grid_size(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(rb_render_call_111258c,label.0,value.as_mut_ptr()); value
    }
    fn grid_extent(&mut self,label: ResourceId)->[u8;2] {
        let mut value = [0;2]; call!(rb_render_call_111299c,label.0,0,value.as_mut_ptr()); value
    }
    fn menu_mode(&mut self)->u32 { call!(rb_render_call_bf96b0) }
    fn setting_help_hidden(&mut self)->bool { call!(rb_render_call_61c93c,0x44,0) != 0 }
    fn metric_footer(&mut self,last: u8) { call!(rb_render_call_1139a68,5,2,last as u32,25,20,18,1); }
    fn resolution_footer(&mut self,node: NodeId,parent: u32,child: u32) { call!(rb_render_call_1159168,node.0,parent,child); }
}
