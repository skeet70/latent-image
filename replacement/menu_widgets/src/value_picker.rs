use crate::render_commands::{DrawReport, FontId, NodeId, ResourceId, RowIndex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LabelField { Main = 1, Heading = 0, Detail = 2, Help = 3 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabelBound { Stock, RecipeBank }
impl LabelBound {
    fn maximum(self) -> u32 { match self { Self::Stock => 0x83d, Self::RecipeBank => 0x849 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawMode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotSelection { Live, SavedSlotTwo }
impl DrawMode {
    pub fn snapshot(self) -> Option<SnapshotSelection> {
        match self.0 {
            2 | 3 => None,
            7 | 9 | 13 => Some(SnapshotSelection::SavedSlotTwo),
            _ => Some(SnapshotSelection::Live),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct Snapshot {
    pub kind: u32, pub reserved: u32, pub layout: u32, pub parent: u32,
    pub selected: u32, pub remembered: u32, pub word6: u32, pub word7: u32,
    pub rows: u32, pub page: u32, pub scroll_top: u32,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct Summary { pub children: u32, pub word1: u32, pub capacity: u32, pub word3: u32, pub word4: u32, pub word5: u32 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct Child { pub id: u32, pub state: u32, pub auxiliary: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct HookRecord {
    pub node: u32, pub parent: u32, pub child: u32, pub side_state: [u32; 5],
    pub x: i32, pub y: i32, pub word10: u32, pub selected: u32, pub label: u32,
    pub reserved: [u32; 3],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ResourceRecord {
    pub label: u32, pub x: u16, pub y: u16, pub width: u16, pub height: u16,
    pub word3: u32, pub word4: u32, pub style: u32, pub flags: u32, pub word7: u32,
}
impl ResourceRecord {
    fn detail(label: ResourceId, x: u16, y: u16) -> Self {
        Self { label: label.0, x, y, width: 0, height: 0, word3: 0, word4: 0, style: 0, flags: 0, word7: 0 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridFill { pub left: i32, pub right: i32, pub row: i32, pub height: u32, pub mode: u32, pub resource: ResourceId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellFill { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32, pub font: FontId, pub palette: u32, pub alpha: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelFill { pub left: i32, pub top: i32, pub right: u32, pub bottom: i32, pub font: FontId, pub palette: u32, pub alpha: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette { pub index: u32, pub alpha: u32, pub component2: u32, pub component3: u32 }
impl Palette { fn ordinary(index: u32) -> Self { Self { index, alpha: 255, component2: 0, component3: 0 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawArgs {
    pub node: NodeId, pub x: u16, pub y: u16, pub width: i16, pub column_step: u32,
    pub row_step: u32, pub style: u32, pub scrolling: bool, pub hide_help: u8,
    pub mode: DrawMode, pub label_bound: LabelBound,
}

pub trait PickerServices {
    fn empty_hook(&mut self) -> HookRecord;
    fn clear_report(&mut self, report: &mut DrawReport);
    fn panel_font(&mut self) -> FontId;
    fn snapshot(&mut self, node: NodeId, selection: SnapshotSelection) -> Snapshot;
    fn summary(&mut self, node: NodeId, parent: u32) -> Summary;
    fn child(&mut self, node: NodeId, parent: u32, index: RowIndex) -> Child;
    fn label(&mut self, node: NodeId, parent: u32, child: u32, field: LabelField) -> ResourceId;
    fn label_columns(&mut self, label: ResourceId, catalog: bool) -> u32;
    fn page_quotient(&mut self, numerator: u32, denominator: u32) -> u32;
    fn side_state(&mut self) -> [u32; 5];
    fn font(&mut self, font: FontId);
    fn palette(&mut self, palette: Palette);
    fn grid_palette(&mut self, palette: u32);
    fn hook(&mut self, record: &mut HookRecord) -> u32;
    fn resource(&mut self, record: &ResourceRecord);
    fn begin_format(&mut self);
    fn end_format(&mut self);
    fn palette_component(&mut self) -> u32;
    fn grid_resource(&mut self, x: i32, y: i32, resource: ResourceId);
    fn grid_fill(&mut self, fill: GridFill);
    fn cell_fill(&mut self, fill: CellFill);
    fn pixel_fill(&mut self, fill: PixelFill);
    fn text_format(&mut self, enabled: bool);
    fn bar_style(&mut self) -> u32;
    fn bar_resources(&mut self, style: u32) -> [ResourceId; 3];
    fn grid_size(&mut self, label: ResourceId) -> [u8; 2];
    fn grid_extent(&mut self, label: ResourceId) -> [u8; 2];
    fn menu_mode(&mut self) -> u32;
    fn setting_help_hidden(&mut self) -> bool;
    fn metric_footer(&mut self, last_column: u8);
    fn resolution_footer(&mut self, node: NodeId, parent: u32, child: u32);
}

fn sb(value: u32) -> i32 { value as u8 as i8 as i32 }
fn sh(value: u16) -> i32 { value as i16 as i32 }
fn pixel(value: u16, scale: u16) -> u16 { value.wrapping_sub(1).wrapping_mul(scale) }
fn detail_draw(s: &mut impl PickerServices, font: u32, palette: u32, record: &ResourceRecord) {
    s.font(FontId(font)); s.grid_palette(palette); s.resource(record);
}

pub fn value_picker(s: &mut impl PickerServices, args: DrawArgs, report: &mut DrawReport) {
    let mut hook = s.empty_hook();
    let saved_font = s.panel_font();
    let Some(selection) = args.mode.snapshot() else { return; };
    let snap = s.snapshot(args.node, selection);
    s.clear_report(report);
    report.put_word(0, snap.kind); report.put_word(4, args.node.0);
    report.0[0x3d] = 255;
    report.put_word(0x20, snap.parent); report.put_word(0x30, snap.scroll_top);
    report.put_word(0x38, snap.remembered); report.put_word(0x28, snap.rows);
    let mut summary = s.summary(args.node, snap.parent);
    if summary.capacity == 0 { summary.capacity = 1; }
    report.put_word(0x2c, if args.node.0 == 0x4c { 3 } else { summary.capacity });
    let mut x = args.x;
    let mut y = args.y;
    if snap.layout == 4 {
        let mut total = 0u32;
        for index in 0..summary.children {
            let child = s.child(args.node, snap.parent, RowIndex(index));
            if child.state != 3 {
                let label = s.label(args.node, snap.parent, child.id, LabelField::Main);
                total = total.wrapping_add(s.label_columns(label, label.0 <= args.label_bound.maximum()));
            }
        }
        x = 17u32.wrapping_sub(total.wrapping_add(2) >> 1) as u16;
    }
    if summary.capacity > snap.rows && args.node.0 != 0x4c {
        let offset = ((summary.capacity - snap.rows) >> 1) as u8;
        report.0[0x3e] = if cfg!(picker_report_offset_wrong) { offset.wrapping_add(1) } else { offset };
        y = y.wrapping_add(args.row_step.wrapping_mul(offset as u32) as u16);
    }
    let mut skip = if args.scrolling && !cfg!(picker_scroll_ignored) { snap.scroll_top } else { 0 };
    let mut ordinal = (0u32.wrapping_sub(skip)) as u8;
    let mut hidden = 0u32;
    let mut visible = 0u32;
    let mut page_count = 0u32;
    let mut extent = 640u32;
    let mut help_start = 0u32;
    for index in 0..summary.children {
        let child = s.child(args.node, snap.parent, RowIndex(index));
        if child.state == 3 && !cfg!(picker_hidden_drawn) { hidden = hidden.wrapping_add(1); }
        else {
            if visible >= summary.capacity { break; }
            if skip != 0 { skip -= 1; }
            else {
                if !args.scrolling {
                    let page = s.page_quotient(page_count.wrapping_add(summary.capacity), summary.capacity);
                    if snap.page > page { page_count = page_count.wrapping_add(1); ordinal = ordinal.wrapping_add(1); continue; }
                    if snap.page < page { break; }
                }
                let label = s.label(args.node, snap.parent, child.id, LabelField::Main);
                let mut columns = s.label_columns(label, label.0 <= 0x83d);
                let selected = child.id == snap.selected;
                hook.node = args.node.0; hook.parent = snap.parent; hook.child = child.id;
                hook.side_state = s.side_state(); hook.x = sh(x); hook.y = sh(y);
                hook.selected = selected as u32; hook.label = label.0;
                let mut resource = ResourceRecord { label: label.0, x: pixel(x, 20), y: pixel(y, 24),
                    width: pixel(args.width as u16, 20), height: pixel(args.row_step as u16, 24),
                    word3: 0, word4: 0, style: args.style, flags: 0, word7: 0 };
                s.font(FontId(12));
                if snap.layout == 4 {
                    s.font(FontId(12));
                    if selected {
                        resource.x = x.wrapping_mul(20);
                        s.begin_format();
                        let component = s.palette_component() as u8 as u32;
                        s.palette(Palette { index: 7, alpha: 255, component2: component, component3: 255 });
                        s.resource(&resource);
                        let right = (x as u8).wrapping_add(columns as u8);
                        s.grid_resource(sb(x as u32), sb(y as u32), ResourceId(0xff00035f));
                        s.grid_resource(sb(right.wrapping_add(1) as u32), sb(y as u32), ResourceId(0xff000360));
                        s.end_format();
                        let style = s.bar_style();
                        s.font(FontId(15));
                        let resources = s.bar_resources(style);
                        s.grid_resource(sb(x as u32), sb(y as u32), resources[0]);
                        s.grid_fill(GridFill { left: sb((x as u8).wrapping_add(1) as u32), right: sb(right as u32), row: sb(y as u32), height: 1, mode: 2, resource: resources[1] });
                        s.grid_resource(sb(right.wrapping_add(1) as u32), sb(y as u32), resources[2]);
                        columns = columns.wrapping_add(2);
                    } else {
                        s.palette(Palette::ordinary(if child.state == 2 { 107 } else { 0 }));
                        s.resource(&resource);
                    }
                } else {
                    s.palette(Palette::ordinary(if selected && !cfg!(picker_selected_unstyled) { 105 } else if child.state == 2 && !cfg!(picker_disabled_offered) { 107 } else { 106 }));
                    if s.hook(&mut hook) == 0 { s.resource(&resource); }
                }
                if matches!(args.node.0, 0x21..=0x23 | 0x28 | 0x4c | 0x83) { extent = 600; }
                if selected && matches!(args.mode.0, 7 | 13) {
                    s.text_format(true); s.font(FontId(13)); s.palette(Palette::ordinary(106));
                    if s.hook(&mut hook) == 0 { s.font(FontId(13)); s.palette(Palette::ordinary(106)); s.resource(&resource); }
                    s.pixel_fill(PixelFill { left: sh(resource.x.wrapping_sub(20)), top: sh(resource.y), right: extent, bottom: sh(resource.y.wrapping_add(48)), font: FontId(15), palette: 124, alpha: 255 });
                    s.text_format(false);
                }
                let position = (ordinal as u32).wrapping_sub(hidden).wrapping_sub(if args.scrolling { 0 } else { summary.capacity.wrapping_mul(snap.page.wrapping_sub(1)) }) as u8;
                if child.id == snap.remembered { report.0[0x3d] = position; }
                if selected {
                    report.put_word(0x34, child.id); report.put_word(0x40, child.auxiliary); report.0[0x3c] = position;
                    if snap.layout == 4 {
                        if s.hook(&mut hook) == 0 { horizontal_details(s, args, snap, child, y, &mut help_start); }
                    } else { vertical_details(s, args, snap, child, pixel(y, 24)); }
                }
                page_count = page_count.wrapping_add(1); visible += 1;
                y = y.wrapping_add(args.row_step as u16);
                x = x.wrapping_add(if args.column_step == 255 { columns } else { args.column_step } as u16);
            }
        }
        ordinal = ordinal.wrapping_add(1);
    }
    s.font(saved_font);
}

fn horizontal_details(s: &mut impl PickerServices, args: DrawArgs, snap: Snapshot, child: Child, y: u16, help_start: &mut u32) {
    let heading = s.label(args.node, snap.parent, child.id, LabelField::Heading);
    let detail = s.label(args.node, snap.parent, child.id, LabelField::Detail);
    let help = s.label(args.node, snap.parent, child.id, LabelField::Help);
    let start = y.wrapping_add(2);
    let mut end = start;
    if heading.0 != 0 {
        let record = ResourceRecord::detail(heading, if detail.0 != 0 { 120 } else { 280 }, y.wrapping_add(1).wrapping_mul(24));
        detail_draw(s, 15, 10, &record);
        end = start.wrapping_add(s.grid_size(heading)[1] as u16);
    }
    if detail.0 != 0 {
        detail_draw(s, 15, 10, &ResourceRecord::detail(detail, 0xffec, 0xffec));
        if heading.0 == 0 { end = end.wrapping_add(s.grid_size(detail)[1] as u16); }
    }
    if args.hide_help == 0 && help.0 != 0 {
        detail_draw(s, 15, 10, &ResourceRecord::detail(help, 0xffec, 0xffec));
        let _ = s.grid_extent(help);
        let size = s.grid_size(help);
        *help_start = end as u32;
        end = end.wrapping_add(size[1] as u16);
    }
    let palette = if s.menu_mode() == 5 { 99 } else { 0 };
    let (left, right) = if matches!(args.node.0, 0x10 | 0x30) { (5, 28) } else { (6, 29) };
    s.palette(Palette { index: palette, alpha: 204, component2: 0, component3: 0 });
    if args.hide_help == 0 && help.0 != 0 {
        s.cell_fill(CellFill { left, top: sb(start as u32), right, bottom: sb(help_start.wrapping_sub(1)), font: FontId(17), palette, alpha: 204 });
        s.palette(Palette { index: palette, alpha: 153, component2: 0, component3: 0 });
        s.cell_fill(CellFill { left, top: sb(*help_start), right, bottom: sb(end.wrapping_sub(1) as u32), font: FontId(17), palette, alpha: 153 });
    } else {
        s.cell_fill(CellFill { left, top: sb(start as u32), right, bottom: sb(end.wrapping_sub(1) as u32), font: FontId(17), palette, alpha: 204 });
    }
    s.font(FontId(17));
    s.grid_fill(GridFill { left: left + 1, right: right - 1, row: sb(end as u32), height: 1, mode: 1, resource: ResourceId(0xff000361) });
    s.grid_resource(left, sb(end as u32), ResourceId(0xff000362));
    s.grid_resource(right, sb(end as u32), ResourceId(0xff000363));
}

fn vertical_details(s: &mut impl PickerServices, args: DrawArgs, snap: Snapshot, child: Child, row_y: u16) {
    if snap.layout == 1 {
        let detail = s.label(args.node, snap.parent, child.id, LabelField::Detail);
        if detail.0 != 0 { detail_draw(s, 13, 10, &ResourceRecord::detail(detail, 0xffec, 0xffec)); }
        let help = s.label(args.node, snap.parent, child.id, LabelField::Help);
        if help.0 != 0 && !s.setting_help_hidden() {
            detail_draw(s, 13, 10, &ResourceRecord::detail(help, if args.node.0 == 6 { 140 } else { 0xffec }, 0xffec));
        }
    } else {
        let help = s.label(args.node, snap.parent, child.id, LabelField::Help);
        if help.0 != 0 {
            let record = ResourceRecord::detail(help, 0xffec, row_y);
            s.font(FontId(15)); s.grid_palette(19); s.begin_format(); s.resource(&record); s.end_format();
        }
        let detail = s.label(args.node, snap.parent, child.id, LabelField::Detail);
        if detail.0 != 0 && !s.setting_help_hidden() {
            detail_draw(s, 15, 10, &ResourceRecord::detail(detail, 0xffec, 0xffec));
            let extent = s.grid_extent(detail);
            s.metric_footer(extent[0].wrapping_sub(1));
        }
        if snap.layout == 9 { s.resolution_footer(args.node, snap.parent, child.id); }
    }
}
