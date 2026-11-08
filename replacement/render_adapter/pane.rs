use crate::menu_widgets::pane_drawer::{DrawMode, DrawServices, Format, Hook, HookRecord, NestedChild, Palette, PaneState, RootChild, TabTable};
use crate::render_commands::{DrawReport, FontId, NodeId, ResourceId};
use core::ptr::read_volatile;
use super::calls::*;

pub(crate) struct StockServices;
unsafe extern "C" {
    fn rb_render_call_1165aa0(node: u32, mode: u32);
    fn rb_render_call_1112b2c(font: u32);
    fn rb_render_call_bf9360(node: u32, index: u32, alternate: u32) -> u32;
    fn rb_render_call_116cf50(node: u32, child: u32) -> u32;
    fn rb_render_call_1163e04(selected: u32, state: u32);
    fn rb_render_call_1163c1c(selected: u32, state: u32);
    fn rb_render_call_bf93e0(node: u32, child: u32, variant: u32, nested: u32) -> u32;
    fn rb_render_call_1112da4();
    fn rb_render_call_1112d84();
    fn rb_render_call_1112e38();
    fn rb_render_call_1112e58();
    fn rb_render_call_1177e10(table: u32, enabled: u32, mode: u32);
    fn rb_render_call_1112ce4(index: u32);
    fn rb_render_call_1137d2c(index: u32);
    fn rb_render_call_1137cf8(index: u32, first: u32, second: u32, third: u32);
    fn rb_render_call_1113514(x: i32, y: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32);
    fn rb_render_call_111278c(x: i32, unused: u32, y: i32, bottom: i32, alignment: u32, mode: u32, resource: u32);
    fn rb_render_call_11121d4(x: i32, y: i32, resource: u32, mode: u32);
    fn rb_render_call_111236c(x: i32, y: i32, resource: u32);
    fn rb_render_call_116d4e4(node: u32, parent: u32, selection: u32);
    fn rb_render_call_1159168(node: u32, parent: u32, selection: u32);
    fn rb_render_call_1112d64(x: i32, y: i32, right: i32, bottom: i32);
    fn rb_render_call_11130e0(x: i32, y: i32, right: u32, bottom: i32, colour: u32, unused: u32, alpha: u32);
    fn rb_render_call_1135db0() -> u32;
    fn rb_render_call_113f1e8() -> u32;
    fn rb_render_call_bf9614() -> u32;
    fn rb_render_call_bf963c() -> u32;
    fn rb_render_call_bf9660() -> u32;
    fn rb_render_call_bf9688() -> u32;
    fn rb_render_call_1130e68(style: u32) -> u32;
    fn rb_render_call_1138a00(index: u32) -> u32;
    fn rb_render_call_112eb38(resource: u32, x: u32, y: u32);
}
macro_rules! stock {
    ($name:ident $(, $argument:expr)* $(,)?) => { unsafe { $name($($argument),*) } };
}
impl DrawServices for StockServices {
    fn prepare(&mut self, node: NodeId, mode: DrawMode) -> PaneState {
        stock!(rb_render_call_1165aa0, node.0, mode.0);
        unsafe { PaneState {
            side: read_volatile(0x015e64d8 as *const [u32; 7]),
            summary: read_volatile(0x015e64f4 as *const [u32; 3]),
            report: read_volatile(0x015e650c as *const DrawReport),
        } }
    }
    fn panel_font(&self) -> FontId { FontId(unsafe { read_volatile(0x015e46c0 as *const u32) }) }
    fn font(&mut self, font: FontId) { stock!(rb_render_call_1112b2c, font.0); }
    fn describe_root(&mut self, node: NodeId, index: u32, out: &mut RootChild) { stock!(rb_render_call_bf8d20, node.0, index, (out as *mut RootChild).cast()); }
    fn describe_nested(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild) { stock!(rb_render_call_bf90f0, node.0, parent, index, (out as *mut NestedChild).cast()); }
    fn label(&mut self, node: NodeId, index: u32, alternate: bool) -> ResourceId { ResourceId(stock!(rb_render_call_bf9360, node.0, index, alternate as u32)) }
    fn special_format(&mut self, node: NodeId, child: u32) -> bool { stock!(rb_render_call_116cf50, node.0, child) != 0 }
    fn label_style(&mut self, selected: bool, state: u32) { stock!(rb_render_call_1163e04, selected as u32, state); }
    fn nested_style(&mut self, selected: bool, state: u32) { stock!(rb_render_call_1163c1c, selected as u32, state); }
    fn value(&mut self, node: NodeId, child: u32, variant: u32, nested: bool) -> ResourceId {
        #[cfg(not(rb_pane_native))]
        if node.0 == 1 && !nested && matches!(child, 0x281 | 0x285) { return ResourceId(0); }
        let resource = ResourceId(stock!(rb_render_call_bf93e0, node.0, child, variant, nested as u32));
        #[cfg(rb_pane_preview_mutant)]
        if node.0 == 1 && !nested && matches!(child, 0x281 | 0x285) { return ResourceId(0xff000001); }
        resource
    }
    fn hook(&mut self, kind: Hook, record: &mut HookRecord) -> u32 {
        let pointer = (record as *mut HookRecord).cast();
        match kind { Hook::Label => stock!(rb_render_call_116dce0, pointer), Hook::Value => stock!(rb_render_call_116e264, pointer), Hook::Nested => stock!(rb_render_call_116f948, pointer) }
    }
    fn format(&mut self, format: Format) {
        match format {
            Format::Ia(false) => stock!(rb_render_call_1112da4), Format::Ia(true) => stock!(rb_render_call_1112d84),
            Format::Brackets(true) => stock!(rb_render_call_1112e38), Format::Brackets(false) => stock!(rb_render_call_1112e58),
            Format::Text(enabled) => stock!(rb_render_call_1177e10, 0x01113bf4, enabled as u32, 0),
        }
    }
    fn palette(&mut self, palette: Palette) {
        match palette { Palette::Primary(index) => stock!(rb_render_call_1112ce4, index), Palette::Grid(index) => stock!(rb_render_call_1137d2c, index), Palette::Components(a,b,c,d) => stock!(rb_render_call_1137cf8, a,b,c,d) }
    }
    fn cell_box(&mut self, x: i32, y: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32) { stock!(rb_render_call_1113514, x,y,right,bottom,stroke,colour,alpha); }
    fn aligned(&mut self, x: i32, y: i32, bottom: i32, alignment: u32, resource: ResourceId) { stock!(rb_render_call_111278c, x,0,y,bottom,alignment,2,resource.0); }
    fn label_grid(&mut self, x: i32, y: i32, resource: ResourceId) { stock!(rb_render_call_11121d4, x,y,resource.0,1); }
    fn grid(&mut self, x: i32, y: i32, resource: ResourceId) { stock!(rb_render_call_111236c, x,y,resource.0); }
    fn detail(&mut self, node: NodeId, parent: u32, selection: u32) { stock!(rb_render_call_116d4e4, node.0,parent,selection); }
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32) { stock!(rb_render_call_1159168, node.0,parent,selection); }
    fn pixel_display(&self) -> bool { unsafe { read_volatile(0x0095676c as *const u32) == 2 } }
    fn wide_display(&self) -> bool { unsafe { read_volatile(0x0095a690 as *const u32) == 2 } }
    fn bounds(&mut self, x: i32, y: i32, right: i32, bottom: i32) { stock!(rb_render_call_1112d64, x,y,right,bottom); }
    fn pixel_box(&mut self, x: i32, y: i32, right: i32, bottom: i32) { stock!(rb_render_call_11130e0, x,y,right as u32,bottom,18,0,255); }
    fn title_suppressed(&mut self) -> bool { stock!(rb_render_call_1135db0) != 0 }
    fn title_resource(&mut self) -> ResourceId { ResourceId(stock!(rb_render_call_113f1e8)) }
    fn tab_getters(&mut self) -> [u32; 4] { [stock!(rb_render_call_bf9614), stock!(rb_render_call_bf963c), stock!(rb_render_call_bf9660), stock!(rb_render_call_bf9688)] }
    fn tab_resource(&self, table: TabTable, bank: u32, index: u32, word: u32) -> ResourceId {
        let (base, stride) = match table { TabTable::Short => (0x0116cd34, 6), TabTable::Long => (0x0116ce9c, 15), TabTable::Extra => (0x0116cde8, 15) };
        ResourceId(unsafe { read_volatile((base + 4 * (bank * stride + index * 3 + word)) as *const u32) })
    }
    fn style_marker(&mut self, nested: bool) -> ResourceId {
        let style = stock!(rb_render_call_1130e68, 0);
        ResourceId(unsafe { read_volatile((0x0110f528 + style * 8 + if nested { 4 } else { 0 }) as *const u32) })
    }
    fn remembered_colour(&mut self) -> u32 { stock!(rb_render_call_1138a00, 8) }
    fn final_text(&mut self) { stock!(rb_render_call_112eb38, 0x78c,1,19); }
}
