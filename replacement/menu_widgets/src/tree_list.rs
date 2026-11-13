use crate::render_commands::{producer::ProducerError, DrawReport, FontId, NodeId, ResourceId, SideState};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Snapshot(pub [u32; 11]);
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Summary { pub rows: u32, pub reserved: u32, pub page_capacity: u32, pub retained: [u32; 3] }
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Child { pub record: u32, pub id: u32, pub selection: u32, pub variant: u32, pub state: u32, pub reserved: u32 }
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HookRecord(pub [u32; 16]);
#[derive(Clone, Copy)]
#[repr(C)]
pub struct ResourceDescriptor {
    pub resource: ResourceId,
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
    pub reserved: [u32; 3],
    pub layout: u32,
    pub tail: u32,
}
#[derive(Clone, Copy)]
pub struct DrawMode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotSource { Live, SavedSlot1, SavedSlot2 }
#[derive(Clone, Copy, Eq, PartialEq)]
enum ListKind { Flat, Tree, Unknown }
impl DrawMode {
    pub fn snapshot_source(self) -> SnapshotSource {
        if self.0 <= 13 && (1u32 << self.0) & 0x2280 != 0 { SnapshotSource::SavedSlot2 }
        else if self.0 <= 13 && (1u32 << self.0) & 0x1860 != 0 { SnapshotSource::SavedSlot1 }
        else { SnapshotSource::Live }
    }
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct NestedChild { pub selection: u32, pub state: u32, pub data: u32 }
#[derive(Clone, Copy, Default)]
pub struct Selection { pub record: u32, pub selection: u32, pub index: u32 }
#[derive(Clone, Copy)]
pub struct NestedGrid { pub first: u16, pub second: u16, pub first_step: u32, pub second_step: u32 }
#[derive(Clone, Copy)]
pub struct DrawArgs {
    pub node: NodeId,
    pub first: u16,
    pub second: u16,
    pub first_step: u32,
    pub second_step: u32,
    pub nested: NestedGrid,
    pub mode: DrawMode,
}

pub trait DrawServices {
    fn initialize_hook(&mut self) -> HookRecord;
    fn panel_font(&self) -> FontId;
    fn snapshot(&mut self, node: NodeId, source: SnapshotSource, out: &mut Snapshot);
    fn resolve_selection(&mut self, node: NodeId, selection: &mut Selection) -> bool;
    fn nested_summary(&mut self, node: NodeId, parent: u32, out: &mut Summary);
    fn nested_child(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild);
    fn nested_label(&mut self, node: NodeId, parent: u32, selection: u32) -> ResourceId;
    fn selection_hook(&mut self, record: &mut HookRecord) -> u32;
    fn selected_detail(&mut self, node: NodeId, parent: u32, selection: u32);
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32);
    fn clear_report(&mut self, out: &mut DrawReport);
    fn summarize(&mut self, node: NodeId, out: &mut Summary);
    fn describe(&mut self, node: NodeId, index: u32, out: &mut Child);
    fn divide(&mut self, numerator: u32, denominator: u32) -> u32;
    fn remainder(&mut self, numerator: u32, denominator: u32) -> u32;
    fn label(&mut self, node: NodeId, index: u32) -> ResourceId;
    fn font(&mut self, font: FontId);
    fn side_state(&mut self, state: SideState);
    fn selection_palette(&mut self, selected: bool, state: u32);
    fn disable_format(&mut self);
    fn enable_format(&mut self);
    fn cell_box(&mut self, first: i32, second: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32);
    fn label_hook(&mut self, record: &mut HookRecord) -> u32;
    fn resource(&mut self, descriptor: &mut ResourceDescriptor);
    fn special_mode(&self) -> u32;
    fn text_format(&mut self, enabled: bool);
    fn descriptor_highlight(&mut self, state: u32);
    fn grid_resource(&mut self, first: u32, second: i32, resource: ResourceId);
    fn palette_override(&mut self, node: NodeId, child: u32) -> u32;
    fn primary_palette(&mut self, palette: u32);
    fn value(&mut self, node: NodeId, child: u32, variant: u32) -> ResourceId;
    fn value_hook(&mut self, record: &mut HookRecord) -> u32;
    fn palette_components(&mut self, index: u32, first: u32, second: u32, third: u32);
    fn grid_rectangle(&mut self, second: i32);
}

fn signed_byte(value: u16) -> i32 { value as u8 as i8 as i32 }

#[inline]
pub fn tree_list<S: DrawServices>(services: &mut S, args: DrawArgs, report: &mut DrawReport) -> Result<(), ProducerError> {
    let mut snapshot = Snapshot([0; 11]);
    let mut hook = services.initialize_hook();
    let saved_font = services.panel_font();
    services.snapshot(args.node, args.mode.snapshot_source(), &mut snapshot);
    let mut s = snapshot.0;
    if (args.node.0 == 0x25 && s[4] == 0 && s[3] == 0x27)
        || (args.node.0 == 0x38 && s[4] == 11 && s[3] == 0x228) { s[0] = 1; }
    let kind = match s[0] { 1 => ListKind::Flat, 2 => ListKind::Tree, _ => ListKind::Unknown };
    services.clear_report(report);
    report.put_word(0, s[0])?;
    report.put_word(4, s[1])?;
    report.0[0x1d] = 255;
    report.0[0x3d] = 255;
    report.put_word(0x20, s[3])?;
    report.put_word(0x30, s[10])?;
    report.put_word(0x38, s[5])?;
    let mut summary = Summary { rows: 0, reserved: 0, page_capacity: 0, retained: [0; 3] };
    services.summarize(args.node, &mut summary);
    let capacity = summary.page_capacity.max(1);
    let mut selection = Selection::default();
    let mut nested_child = NestedChild { selection: 0, state: 0, data: 0 };
    let (selected_index, page, count) = if kind == ListKind::Flat {
        report.put_word(8, s[3])?;
        report.put_word(0x10, s[4])?;
        report.put_word(0x14, s[8] as u8 as u32)?;
        report.put_word(0x18, capacity)?;
        (s[7], s[9], s[8])
    } else if kind == ListKind::Tree {
        let mut count = 0u32;
        let mut hidden = 0u32;
        let mut child = Child { record: 0, id: 0, selection: 0, variant: 0, state: 0, reserved: 0 };
        for raw in 0..summary.rows {
            services.describe(args.node, raw, &mut child);
            if child.state == 3 { hidden = hidden.wrapping_add(1); continue; }
            if services.resolve_selection(args.node, &mut selection) {
                report.put_word(8, selection.record)?;
                report.put_word(0x10, selection.selection)?;
            } else if child.id == s[3] {
                selection.index = raw.wrapping_sub(hidden);
                report.put_word(8, child.record)?;
                report.put_word(0x10, child.selection)?;
            }
            if child.state != 3 { count = count.wrapping_add(1); }
        }
        (selection.index, services.divide(selection.index, capacity).wrapping_add(1), count)
    } else { (0, 0, 0) };
    let page = page.wrapping_add(cfg!(tree_page_off_by_one) as u32);
    if kind != ListKind::Unknown { report.0[0x1f] = page as u8; }
    let mut second = args.second;
    if args.node.0 != 0x53 && args.node.0 != 0x57 && count < capacity {
        let center = (capacity.wrapping_sub(count) >> 1) as u8;
        report.0[0x1e] = center;
        second = second.wrapping_add(args.second_step.wrapping_mul(center as u32) as u16);
    }
    let mut first = args.first;
    let mut visible = 0u8;
    let mut drawn = 0u32;
    let mut child = Child { record: 0, id: 0, selection: 0, variant: 0, state: 0, reserved: 0 };
    let mut descriptor = ResourceDescriptor { resource: ResourceId(0), x: 0, y: 0, width: 0, height: 0, reserved: [0; 3], layout: 0, tail: 0 };
    for raw in 0..summary.rows {
        services.describe(args.node, raw, &mut child);
        if child.state == 3 { continue; }
        let row_page = services.divide((visible as u32).wrapping_add(capacity), capacity);
        if page > row_page { visible = visible.wrapping_add(1); continue; }
        if page < row_page { break; }
        let selected = visible as u32 == selected_index && !cfg!(tree_selected_unstyled);
        if selected {
            report.put_word(0xc, child.id)?;
            report.0[0x1c] = if row_page > 1 { services.remainder(visible as u32, capacity) as u8 } else { drawn as u8 };
        }
        if child.selection == s[5] { report.0[0x1d] = visible; }
        let label = services.label(args.node, raw);
        descriptor.resource = label;
        descriptor.x = first.wrapping_sub(1).wrapping_mul(20) as i16;
        let pixel_y = second.wrapping_sub(1).wrapping_mul(24) as i16;
        descriptor.y = pixel_y;
        descriptor.width = 0;
        descriptor.height = 0;
        descriptor.reserved[2] = 0;
        descriptor.layout = 0;
        let alternate = kind == ListKind::Tree || matches!(args.mode.0, 5 | 6 | 11 | 12);
        services.font(FontId(if alternate { 15 } else { 13 }));
        services.side_state(SideState { kind: 3, alternate_font: alternate ^ cfg!(tree_side_state_wrong) });
        services.selection_palette(selected, child.state);
        let format_mode = matches!(args.mode.0, 11 | 12);
        if format_mode && selected {
            services.disable_format();
            services.cell_box(signed_byte(first.wrapping_sub(1)), signed_byte(second), 30, signed_byte(second.wrapping_add(1)), 17, 56, 153);
        }
        hook.0[0] = args.node.0;
        hook.0[1] = child.id;
        hook.0[2] = child.variant;
        hook.0[8] = first as i16 as i32 as u32;
        hook.0[9] = second as i16 as i32 as u32;
        hook.0[10] = 0;
        hook.0[11] = selected as u32;
        hook.0[12] = label.0;
        hook.0[13] = s[0];
        hook.0[15] = raw;
        if services.label_hook(&mut hook) == 0 { services.resource(&mut descriptor); }
        if args.node.0 == 0x38 && s[4] == 11 && s[3] == 0x228 && selected && (args.mode.0 == 13 || services.special_mode() == 13) {
            services.text_format(true);
            services.descriptor_highlight(0);
            services.grid_resource(13, signed_byte(second), label);
            services.cell_box(signed_byte(first.wrapping_sub(1)), signed_byte(second), 31, signed_byte(second.wrapping_add(1)), 15, 124, 255);
            services.text_format(false);
        }
        if format_mode {
            if selected { services.enable_format(); }
        } else {
            if services.palette_override(args.node, child.id) != 0 { services.primary_palette(8); }
            let value = services.value(args.node, child.id, child.variant);
            descriptor.resource = value;
            descriptor.x = -20;
            descriptor.y = pixel_y;
            descriptor.width = 0;
            descriptor.height = pixel_y.wrapping_add(48);
            descriptor.reserved[2] = 0;
            descriptor.layout = 2;
            services.font(FontId(if kind == ListKind::Tree { 15 } else { 13 }));
            hook.0[0] = args.node.0;
            hook.0[1] = child.id;
            hook.0[2] = child.variant;
            hook.0[8] = first as i16 as i32 as u32;
            hook.0[9] = second as i16 as i32 as u32;
            hook.0[10] = 0;
            hook.0[11] = selected as u32;
            hook.0[12] = value.0;
            hook.0[14] = args.mode.0;
            if services.value_hook(&mut hook) == 0 {
                services.resource(&mut descriptor);
                if kind == ListKind::Flat && descriptor.resource.0 != 0 && !matches!(args.mode.0, 5 | 6) {
                    services.font(FontId(16));
                    services.palette_components(28, 204, 107, 204);
                    services.grid_rectangle(signed_byte(second));
                }
            }
        }
        visible = visible.wrapping_add(1);
        drawn = drawn.wrapping_add(1);
        second = second.wrapping_add(args.second_step as u16);
        first = first.wrapping_add(args.first_step as u16);
    }
    if kind == ListKind::Tree && !matches!(args.mode.0, 2 | 3) {
        let mut nested = Summary { rows: 0, reserved: 0, page_capacity: 0, retained: [0; 3] };
        services.nested_summary(args.node, s[3], &mut nested);
        report.put_word(0x20, s[3])?;
        report.put_word(0x24, nested.rows)?;
        report.put_word(0x28, s[8])?;
        report.put_word(0x2c, nested.page_capacity)?;
        report.put_word(0x30, s[10])?;
        report.put_word(0x38, s[5])?;
        report.put_word(0x34, nested_child.selection)?;
        let mut second = args.nested.second;
        if s[8] < nested.page_capacity {
            let center = (nested.page_capacity.wrapping_sub(s[8]) >> 1) as u8;
            report.0[0x3e] = center;
            second = second.wrapping_add(args.nested.second_step.wrapping_mul(center as u32) as u16);
        }
        let mut first = args.nested.first;
        let mut skip = s[10];
        let mut offset = (0u32.wrapping_sub(skip)) as u8;
        let mut hidden = 0u32;
        let mut drawn = 0u8;
        for raw in 0..nested.rows {
            if drawn as u32 >= nested.page_capacity { break; }
            services.nested_child(args.node, s[3], raw, &mut nested_child);
            if nested_child.state == 3 {
                hidden = hidden.wrapping_add(1);
            } else if skip != 0 {
                skip = skip.wrapping_sub(1);
            } else {
                let index = offset.wrapping_sub(hidden as u8);
                if nested_child.selection == s[5] { report.0[0x3d] = index; }
                let selected = nested_child.selection == s[4];
                if selected {
                    report.0[0x3c] = index;
                    report.put_word(0x40, nested_child.data)?;
                }
                let label = services.nested_label(args.node, s[3], nested_child.selection);
                descriptor.resource = label;
                descriptor.x = first.wrapping_sub(1).wrapping_mul(20) as i16;
                descriptor.y = second.wrapping_sub(1).wrapping_mul(24) as i16;
                descriptor.width = 0;
                descriptor.height = 0;
                descriptor.reserved[2] = 0;
                descriptor.layout = 0;
                services.font(FontId(11));
                services.palette_components(if selected { 105 } else if nested_child.state == 2 { 107 } else { 106 }, 255, 0, 0);
                hook.0[0] = args.node.0;
                hook.0[1] = s[3];
                hook.0[2] = nested_child.selection;
                hook.0[8] = first as i16 as i32 as u32;
                hook.0[9] = second as i16 as i32 as u32;
                hook.0[11] = selected as u32;
                hook.0[12] = label.0;
                let handled = services.selection_hook(&mut hook) != 0;
                if !handled { services.resource(&mut descriptor); }
                if selected {
                    if matches!(args.mode.0, 7 | 13) || services.special_mode() == 13 {
                        services.text_format(true);
                        if handled { hook.0[11] = 0; }
                        services.descriptor_highlight(nested_child.state);
                        if handled { services.selection_hook(&mut hook); }
                        else { services.grid_resource(if args.node.0 == 0x38 { 21 } else { 14 }, signed_byte(second), label); }
                        services.cell_box(signed_byte(first.wrapping_sub(1)), signed_byte(second), 32, signed_byte(second.wrapping_add(1)), 15, 124, 255);
                        services.text_format(false);
                    }
                    if !matches!(services.special_mode(), 2 | 3) { services.selected_detail(args.node, s[3], s[4]); }
                    services.footer(args.node, s[3], s[4]);
                }
                drawn = drawn.wrapping_add(1);
                second = second.wrapping_add(args.nested.second_step as u16);
                first = first.wrapping_add(args.nested.first_step as u16);
            }
            offset = offset.wrapping_add(1);
        }
    }
    services.font(saved_font);
    Ok(())
}
