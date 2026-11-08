#[path = "../src/lib.rs"]
mod widgets;
pub use widgets::render_commands;
use render_commands::{DrawReport, FontId, NodeId, ResourceId, RowIndex};
use widgets::value_picker::*;

#[derive(Default)]
struct Frame {
    snapshot: Snapshot,
    summary: Summary,
    children: Vec<Child>,
    drawn: Vec<ResourceRecord>,
    font: u32,
    palette: u32,
    colors: Vec<u32>,
    hook_draws: bool,
    hide_help: bool,
}
impl PickerServices for Frame {
    fn empty_hook(&mut self) -> HookRecord {
        HookRecord { node: 0, parent: 0, child: 0, side_state: [0; 5], x: 0, y: 0, word10: 0, selected: 0, label: 0, reserved: [0; 3] }
    }
    fn clear_report(&mut self, report: &mut DrawReport) { *report = DrawReport::zeroed(); }
    fn panel_font(&mut self) -> FontId { FontId(self.font) }
    fn snapshot(&mut self, _: NodeId, _: bool) -> Snapshot { self.snapshot }
    fn summary(&mut self, _: NodeId, _: u32) -> Summary { self.summary }
    fn child(&mut self, _: NodeId, _: u32, index: RowIndex) -> Child { self.children[index.0 as usize] }
    fn label(&mut self, _: NodeId, _: u32, child: u32, field: LabelField) -> ResourceId { ResourceId(if field == LabelField::Main { child } else { 0 }) }
    fn label_columns(&mut self, _: ResourceId, catalog: bool) -> u32 { if catalog { 2 } else { 4 } }
    fn page_quotient(&mut self, n: u32, d: u32) -> u32 { n / d }
    fn side_state(&mut self) -> [u32; 5] { [3, 0, 1, 0, 0] }
    fn font(&mut self, font: FontId) { self.font = font.0; }
    fn palette(&mut self, p: Palette) { self.palette = p.index; }
    fn grid_palette(&mut self, p: u32) { self.palette = p; }
    fn hook(&mut self, _: &mut HookRecord) -> u32 { if self.hook_draws { 7 } else { 0 } }
    fn resource(&mut self, r: &ResourceRecord) { self.drawn.push(*r); self.colors.push(self.palette); }
    fn begin_format(&mut self) {}
    fn end_format(&mut self) {}
    fn palette_component(&mut self) -> u32 { 0 }
    fn grid_resource(&mut self, _: i32, _: i32, _: ResourceId) {}
    fn grid_fill(&mut self, _: GridFill) {}
    fn cell_fill(&mut self, _: CellFill) {}
    fn pixel_fill(&mut self, _: PixelFill) {}
    fn text_format(&mut self, _: bool) {}
    fn bar_style(&mut self) -> u32 { 0 }
    fn bar_resources(&mut self, _: u32) -> [ResourceId; 3] { [ResourceId(0); 3] }
    fn grid_size(&mut self, _: ResourceId) -> [u8; 2] { [2, 1] }
    fn grid_extent(&mut self, _: ResourceId) -> [u8; 2] { [2, 1] }
    fn menu_mode(&mut self) -> u32 { 0 }
    fn setting_help_hidden(&mut self) -> bool { self.hide_help }
    fn metric_footer(&mut self, _: u8) {}
    fn resolution_footer(&mut self, _: NodeId, _: u32, _: u32) {}
}
fn args() -> DrawArgs {
    DrawArgs { node: NodeId(6), x: 2, y: 4, width: 0, column_step: 0, row_step: 2,
        style: 0, scrolling: true, hide_help: 1, mode: DrawMode(0), label_bound: LabelBound::Stock }
}
fn frame() -> Frame {
    Frame { snapshot: Snapshot { layout: 1, parent: 17, selected: 12, remembered: 11, rows: 3, page: 1, ..Snapshot::default() },
        summary: Summary { children: 4, capacity: 3, ..Summary::default() },
        children: vec![Child { id: 10, state: 3, auxiliary: 100 }, Child { id: 11, state: 0, auxiliary: 101 }, Child { id: 12, state: 0, auxiliary: 102 }, Child { id: 13, state: 2, auxiliary: 103 }],
        font: 99, ..Frame::default() }
}
#[test]
fn visible_rows_report_selection_and_restore_font() {
    let mut frame = frame(); let mut report = DrawReport([0xaa; 68]);
    value_picker(&mut frame, args(), &mut report);
    assert_eq!(frame.drawn.iter().map(|r| (r.label,r.x,r.y)).collect::<Vec<_>>(), vec![(11,20,72),(12,20,120),(13,20,168)]);
    assert_eq!(frame.colors, vec![106,105,107]);
    assert_eq!(frame.font, 99);
    assert_eq!(&report.0[0x34..0x38], &12u32.to_le_bytes());
    assert_eq!(&report.0[0x40..0x44], &102u32.to_le_bytes());
    assert_eq!(&report.0[0x3c..0x40], &[1,0,0,0]);
}
#[test]
fn scroll_skips_nonhidden_rows_without_moving_first_drawn_row() {
    let mut frame = frame(); frame.snapshot.scroll_top = 1;
    let mut report = DrawReport::zeroed();
    value_picker(&mut frame,args(),&mut report);
    assert_eq!(frame.drawn.iter().map(|r| (r.label,r.y)).collect::<Vec<_>>(),vec![(12,72),(13,120)]);
    assert_eq!(report.0[0x3c],0);
    assert_eq!(report.0[0x3d],255);
}
#[test]
fn nonzero_hook_result_owns_the_resource_draw() {
    let mut frame = frame(); frame.hook_draws = true;
    let mut report = DrawReport::zeroed(); value_picker(&mut frame,args(),&mut report);
    assert!(frame.drawn.is_empty());
    assert_eq!(&report.0[0x34..0x38], &12u32.to_le_bytes());
    assert_eq!(frame.font,99);
}
#[test]
fn skipped_modes_preserve_the_report() {
    for mode in [2,3] {
        let mut frame = frame(); let mut args = args(); args.mode = DrawMode(mode);
        let mut report = DrawReport([0xaa;68]); value_picker(&mut frame,args,&mut report);
        assert_eq!(report,DrawReport([0xaa;68])); assert!(frame.drawn.is_empty()); assert_eq!(frame.font,99);
    }
}

#[test]
fn horizontal_centering_respects_the_installed_prepass_label_bound() {
    for (label, bound, expected_x) in [
        (0x83d, LabelBound::Stock, 280),
        (0x83e, LabelBound::Stock, 260),
        (0x83e, LabelBound::RecipeBank, 280),
        (0x849, LabelBound::RecipeBank, 280),
        (0x84a, LabelBound::RecipeBank, 260),
    ] {
        let mut frame = frame();
        frame.snapshot.layout = 4; frame.snapshot.selected = 0; frame.snapshot.rows = 1;
        frame.summary.children = 1; frame.summary.capacity = 1;
        frame.children = vec![Child { id: label, state: 0, auxiliary: 0 }];
        let mut args = args(); args.label_bound = bound;
        let mut report = DrawReport::zeroed(); value_picker(&mut frame,args,&mut report);
        assert_eq!(frame.drawn.iter().map(|r| (r.label,r.x)).collect::<Vec<_>>(),vec![(label,expected_x)]);
        assert_eq!(frame.font,99);
    }
}
