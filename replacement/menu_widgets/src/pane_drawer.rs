use crate::render_commands::{DrawReport, FontId, NodeId, ResourceId};

#[derive(Clone, Copy)]
pub struct DrawMode(pub u32);
#[derive(Clone, Copy)]
pub struct PaneState {
    pub side: [u32; 7],
    pub summary: [u32; 3],
    pub report: DrawReport,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct RootChild {
    pub record: u32,
    pub child: u32,
    pub selection: u32,
    pub variant: u32,
    pub state: u32,
    pub suppress_value: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct NestedChild { pub selection: u32, pub state: u32, pub resource_data: u32 }
#[repr(C)]
pub struct HookRecord(pub [u32; 13]);
#[derive(Clone, Copy)]
pub enum Hook { Label, Value, Nested }
#[derive(Clone, Copy)]
pub enum Palette { Primary(u32), Grid(u32), Components(u32, u32, u32, u32) }
#[derive(Clone, Copy)]
pub enum Format { Ia(bool), Brackets(bool), Text(bool) }
#[derive(Clone, Copy)]
pub enum TabTable { Short, Long, Extra }

pub trait DrawServices {
    fn prepare(&mut self, node: NodeId, mode: DrawMode) -> PaneState;
    fn panel_font(&self) -> FontId;
    fn font(&mut self, font: FontId);
    fn describe_root(&mut self, node: NodeId, index: u32, out: &mut RootChild);
    fn describe_nested(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild);
    fn label(&mut self, node: NodeId, index: u32, alternate: bool) -> ResourceId;
    fn special_format(&mut self, node: NodeId, child: u32) -> bool;
    fn label_style(&mut self, selected: bool, state: u32);
    fn nested_style(&mut self, selected: bool, state: u32);
    fn value(&mut self, node: NodeId, child: u32, variant: u32, nested: bool) -> ResourceId;
    fn hook(&mut self, kind: Hook, record: &mut HookRecord) -> u32;
    fn format(&mut self, format: Format);
    fn palette(&mut self, palette: Palette);
    fn cell_box(&mut self, x: i32, y: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32);
    fn aligned(&mut self, x: i32, y: i32, bottom: i32, alignment: u32, resource: ResourceId);
    fn label_grid(&mut self, x: i32, y: i32, resource: ResourceId);
    fn grid(&mut self, x: i32, y: i32, resource: ResourceId);
    fn detail(&mut self, node: NodeId, parent: u32, selection: u32);
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32);
    fn pixel_display(&self) -> bool;
    fn wide_display(&self) -> bool;
    fn bounds(&mut self, x: i32, y: i32, right: i32, bottom: i32);
    fn pixel_box(&mut self, x: i32, y: i32, right: i32, bottom: i32);
    fn title_suppressed(&mut self) -> bool;
    fn title_resource(&mut self) -> ResourceId;
    fn tab_getters(&mut self) -> [u32; 4];
    fn tab_resource(&self, table: TabTable, bank: u32, index: u32, word: u32) -> ResourceId;
    fn style_marker(&mut self, nested: bool) -> ResourceId;
    fn remembered_colour(&mut self) -> u32;
    fn final_text(&mut self);
}

fn word(report: &DrawReport, offset: usize) -> u32 {
    u32::from_le_bytes([report.0[offset], report.0[offset + 1], report.0[offset + 2], report.0[offset + 3]])
}
fn signed(value: u8) -> i32 { value as i8 as i32 }
fn hook_record(hook: &mut HookRecord, node: NodeId, child: u32, side: &[u32; 7], x: i32, y: u8, selected: bool, resource: ResourceId) {
    hook.0[0] = node.0;
    hook.0[1] = child;
    hook.0[3..8].copy_from_slice(&side[..5]);
    hook.0[8] = x as u32;
    hook.0[9] = signed(y) as u32;
    hook.0[11] = selected as u32;
    hook.0[12] = resource.0;
}

pub fn pane_drawer<S: DrawServices>(services: &mut S, node: NodeId, mode: DrawMode) {
    let state = services.prepare(node, mode);
    let d = &state.side;
    let r = &state.report;
    let root = NodeId(word(r, 4));
    let mut hook = HookRecord([0; 13]);
    if d[1] != 3 {
        match word(r, 0) {
            1 => services.font(FontId(13)),
            0 | 2 => services.font(FontId(15)),
            _ => (),
        }
        let page = (r.0[0x1f] as u32).wrapping_add(cfg!(pane_page_off_by_one) as u32);
        let mut skip = state.summary[2].wrapping_mul(page) as u8;
        let mut drawn = 0u8;
        let mut y = 4u8;
        let mut child = RootChild::default();
        for index in 0..state.summary[0] {
            if state.summary[2] == 0 || drawn as u32 >= state.summary[2] { break; }
            services.describe_root(root, index, &mut child);
            if child.state == 3 && !cfg!(pane_hidden_drawn) { continue; }
            if skip != 0 { skip = skip.wrapping_sub(1); continue; }
            let selected = child.child == word(r, 0xc) && !cfg!(pane_selected_unstyled);
            let alternate = d[1] == 2 && !selected;
            let label = services.label(root, index, alternate);
            services.label_style(selected, child.state);
            let special = services.special_format(root, child.child);
            if matches!(mode.0, 11 | 12) && selected {
                services.format(Format::Ia(false));
                services.cell_box(5, signed(y), 32, signed(y.wrapping_add(1)), 17, 56, 153);
            }
            hook_record(&mut hook, root, child.child, d, 6, y, selected, label);
            hook.0[10] = 0;
            if services.hook(Hook::Label, &mut hook) == 0 {
                if alternate {
                    if special { services.palette(Palette::Primary(8)); }
                    services.aligned(6, signed(y), signed(y.wrapping_add(1)), 0, label);
                } else { services.label_grid(6, signed(y), label); }
            }
            if matches!(mode.0, 11 | 12) && selected { services.format(Format::Ia(true)); }
            if d[1] != 1 && !(selected && d[0] == 2) {
                let value = services.value(root, child.child, child.variant, false);
                let (x, alignment) = if alternate { (9, 1) } else { (-20, 0) };
                let palette = if d[2] == 0 {
                    if selected { Palette::Components(120,255,0,0) }
                    else if child.state == 2 { Palette::Components(107,255,0,0) }
                    else { Palette::Grid(104) }
                } else if matches!(d[0], 2 | 3) {
                    if selected { Palette::Components(121,255,0,0) }
                    else if child.state == 2 { Palette::Components(107,76,0,0) }
                    else { Palette::Components(118,255,0,0) }
                } else if child.state == 2 { Palette::Components(122,255,0,0) }
                else { Palette::Components(118,255,0,0) };
                services.palette(palette);
                let brackets = special && (d[0] == 0 || (d[0] == 3 && d[1] == 0));
                if special {
                    if brackets {
                        services.format(Format::Brackets(true));
                        services.palette(Palette::Grid(113));
                    } else { services.palette(Palette::Primary(8)); }
                }
                hook_record(&mut hook, root, child.child, d, x, y, selected, value);
                hook.0[2] = child.variant;
                hook.0[10] = alignment;
                let handled = services.hook(Hook::Value, &mut hook);
                if child.suppress_value == 0 && value.0 != 0 && handled == 0 {
                    services.aligned(x, signed(y), signed(y.wrapping_add(1)), alignment, value);
                }
                if brackets { services.format(Format::Brackets(false)); }
            }
            drawn = drawn.wrapping_add(1);
            y = y.wrapping_add(2);
        }
    }
    if d[3] != 2 && !matches!(mode.0, 2 | 3) {
        services.font(FontId(11));
        let mut skip = if cfg!(pane_scroll_ignored) { 0 } else { word(r, 0x30) as u8 };
        let mut drawn = 0u8;
        let mut y = r.0[0x3e].wrapping_mul(2).wrapping_add(3);
        let parent = word(r, 0x20);
        let mut child = NestedChild::default();
        for index in 0..word(r, 0x24) {
            if drawn as u32 >= word(r, 0x2c) { break; }
            services.describe_nested(root, parent, index, &mut child);
            if child.state == 3 && !cfg!(pane_hidden_drawn) { continue; }
            if skip != 0 { skip = skip.wrapping_sub(1); continue; }
            let selected = child.selection == word(r, 0x34) && !cfg!(pane_selected_unstyled);
            let resource = services.value(root, parent, child.selection, true);
            services.nested_style(selected, child.state);
            hook_record(&mut hook, root, parent, d, 13, y, selected, resource);
            hook.0[2] = child.selection;
            let handled = services.hook(Hook::Nested, &mut hook);
            if handled == 0 { services.grid(14, signed(y), resource); }
            if selected {
                if matches!(mode.0, 7 | 13) {
                    services.format(Format::Text(true));
                    services.nested_style(false, child.state);
                    if handled == 0 { services.grid(14, signed(y), resource); }
                    else {
                        hook.0[11] = 0;
                        services.hook(Hook::Nested, &mut hook);
                    }
                    services.format(Format::Text(false));
                }
                services.detail(root, parent, word(r, 0x34));
                services.footer(root, parent, word(r, 0x34));
            }
            drawn = drawn.wrapping_add(1);
            y = y.wrapping_add(2);
        }
    }
    chrome(services, &state, mode);
}

fn chrome<S: DrawServices>(services: &mut S, state: &PaneState, mode: DrawMode) {
    let d = &state.side;
    let r = &state.report;
    if d[0] == 1 {
        if services.pixel_display() {
            let (x,y,right,bottom) = if services.wide_display() { (-160,-30,800,510) } else { (0,0,960,540) };
            services.bounds(x,y,960,540);
            services.pixel_box(x,y,right,bottom);
            services.bounds(0,0,0,0);
        } else { services.cell_box(1,1,32,20,18,0,255); }
    }
    let saved = services.panel_font();
    if !services.title_suppressed() {
        if d[4] == 0 {
            services.font(FontId(18));
            services.cell_box(1,1,32,20,18,0,255);
        }
        services.font(FontId(15));
        services.palette(Palette::Components(109,255,0,0));
        services.grid(9,1,ResourceId(0x27a));
        services.font(FontId(15));
        services.palette(Palette::Grid(6));
        let title = services.title_resource();
        services.grid(5,1,title);
        services.font(saved);
    }
    let getters = services.tab_getters();
    if d[0] != 3 {
        services.font(FontId(15));
        let bank = if d[0] == 0 { 1 } else if d[0] == 2 { 2 } else { 0 };
        let (table,count) = if getters[1] == 0x2c { (TabTable::Short,1) } else { (TabTable::Long,5) };
        let mut y = 3;
        for index in 0..count {
            let resource = services.tab_resource(table,bank,index,if index == r.0[0x1f] as u32 { 0 } else { 1 });
            services.grid(1,y,resource);
            y += 2;
        }
        for index in 0..3 {
            let forced = getters[3] == 1 || (getters[2] != 0 && getters[1] == 0x1f);
            let resource = services.tab_resource(TabTable::Extra,bank,index,if forced { 2 } else { 1 });
            services.grid(1,y,resource);
            y += 2;
        }
    }
    if d[1] != 3 {
        services.font(FontId(17));
        services.grid(4,3,ResourceId(if d[2] == 0 { 0xff0000e3 } else if matches!(d[0],2|3) { 0xff0000e7 } else { 0xff0000e2 }));
        if state.summary[1] > 1 && d[2] == 0 {
            services.font(FontId(16));
            services.palette(Palette::Components(116,255,0,0));
            services.grid(18,18,ResourceId(0xff000617));
            services.grid(18,3,ResourceId(0xff000660));
        }
    }
    if d[3] != 2 {
        if matches!(mode.0,2|3) { services.format(Format::Ia(true)); }
        services.font(FontId(14));
        services.grid(12,2,ResourceId(0xff0000e8));
        if matches!(mode.0,2|3) { services.format(Format::Ia(false)); }
        if !matches!(mode.0,1|2|3) && d[1] != 3 {
            services.font(FontId(13));
            services.palette(Palette::Grid(116));
            services.grid(12,9,ResourceId(0xff00035f));
        }
    }
    let saved = services.panel_font();
    if d[0] != 1 {
        let root_y = signed(r.0[0x1c].wrapping_add(2).wrapping_mul(2));
        if d[3] == 2 && d[1] == 0 {
            services.font(FontId(16));
            let marker = services.style_marker(false);
            services.grid(5,root_y,marker);
            services.font(FontId(13));
            services.palette(Palette::Components(117,255,0,0));
            if d[0] != 3 { services.grid(5,root_y,ResourceId(0xff00062f)); }
            services.grid(32,root_y,ResourceId(0xff00062e));
        } else if matches!(d[1],1|2) || (d[3] != 2 && d[1] != 3) {
            services.font(FontId(16));
            services.grid(5,root_y,ResourceId(0xff0000f3));
        }
        if d[3] != 2 && !matches!(mode.0,2|3) {
            let current_y = r.0[0x3c].wrapping_add(r.0[0x3e]).wrapping_mul(2).wrapping_add(3);
            let remembered_y = r.0[0x3d].wrapping_add(r.0[0x3e]).wrapping_mul(2).wrapping_add(3);
            services.font(FontId(13));
            if r.0[0x3c] != 255 {
                let marker = services.style_marker(true);
                services.grid(13,signed(current_y),marker);
            }
            let colour = services.remembered_colour();
            if r.0[0x3d] != 255 {
                let colour = if current_y == remembered_y && !matches!(mode.0,7|13) { 7 } else { colour };
                services.cell_box(13,signed(remembered_y),13,signed(remembered_y.wrapping_add(1)),11,colour,255);
            }
            if matches!(mode.0,7|13) {
                services.format(Format::Text(true));
                services.cell_box(13,signed(current_y),32,signed(current_y.wrapping_add(1)),15,124,255);
                services.format(Format::Text(false));
            }
        }
        services.font(saved);
    }
    if d[3] != 2 && word(r,0x28) > word(r,0x2c) {
        services.font(FontId(13));
        services.palette(Palette::Components(116,255,0,0));
        services.grid(23,2,ResourceId(0xff000660));
        services.grid(23,17,ResourceId(0xff000617));
    }
    if matches!(d[0],0|1) || (d[0] == 3 && d[1] == 0) { services.final_text(); }
}
