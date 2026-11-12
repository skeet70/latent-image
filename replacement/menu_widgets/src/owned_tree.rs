use crate::render_commands::commands::{Alpha, Colour, DrawCommand, ShapeSecondarySource, TextRef};
use crate::render_commands::producer::{stock_grid_origin, CommandBuffer, ProducerError, ResourceContent, SemanticPrerequisite};
use crate::render_commands::context::OutputBatch;
use crate::render_commands::actor::TaskActor;
use crate::render_commands::raster::RasterResources;
use crate::render_commands::resources::{BitmapCatalog, IndexedItemPalette, TextBinding};
use crate::render_commands::{DrawReport, FontId, NodeId, PaletteIndex, PanelPoint, ResourceId, SideState};
use super::owned_frame::{queue_panel, FrameError};
use super::tree_list::{self, Child, DrawArgs, DrawServices, HookRecord, NestedChild, ResourceDescriptor, Selection, Snapshot, SnapshotSource, Summary};

#[derive(Clone, Copy)]
pub enum HookSemantics { Absent, Unresolved }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChildId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionId(pub u32);


#[derive(Clone, Copy)]
pub enum SpecialMode { Ordinary, Two, Three, Thirteen }
#[derive(Clone, Copy)]
pub struct TreeRow {
    pub child: Child,
    pub label: ResourceId,
    pub value: ResourceId,
    pub hook: HookSemantics,
    pub palette_override: bool,
}

#[derive(Clone, Copy)]
pub enum SelectionResolution {
    FromRows,
    Resolved(Selection),
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookStage { Label, Value, Selection }

#[derive(Clone, Copy)]
pub enum HookDisposition { Default, Handled }

pub struct ResolvedHook<'a, 'text> {
    pub stage: HookStage,
    pub input: HookRecord,
    pub output: HookRecord,
    pub input_font: FontId,
    pub input_palette: IndexedItemPalette,
    pub input_primary_enabled: bool,
    pub output_font: FontId,
    pub output_palette: IndexedItemPalette,
    pub output_primary_enabled: bool,
    pub disposition: HookDisposition,
    pub commands: &'a [DrawCommand<'text>],
}

pub struct NestedRow<'a, 'text> {
    pub child: NestedChild,
    pub label: ResourceId,
    pub hook: HookSemantics,
    pub detail: Option<&'a [DrawCommand<'text>]>,
    pub footer: Option<&'a [DrawCommand<'text>]>,
}

pub struct NestedData<'a, 'text> {
    pub parent: ChildId,
    pub summary: Summary,
    pub rows: &'a [NestedRow<'a, 'text>],
}

#[derive(Clone, Copy)]
pub struct ResourcePlacement {
    pub resource: ResourceId,
    pub position: [i16; 2],
    pub extent: [i16; 2],
}

#[derive(Clone, Copy)]
enum Alignment { Natural, Start, Centre, End }

impl Alignment {
    fn parse(value: u32) -> Result<Self, ProducerError> {
        match value {
            0 => Ok(Self::Natural), 1 => Ok(Self::Start), 2 => Ok(Self::Centre), 3 => Ok(Self::End),
            _ => Err(ProducerError::MissingSemantic(SemanticPrerequisite::Alignment)),
        }
    }
    fn coordinate(self, origin: i16, end: i16, extent: i16, natural: i16, catalog: bool) -> i16 {
        let available = (end as u16).wrapping_sub(origin as u16) as i16;
        match self {
            Self::Natural if catalog && origin == -20 => natural,
            Self::Centre => ((origin as u16 as i32) - i32::from(extent) / 2 + i32::from(available) / 2) as i16,
            Self::End => (i32::from(available) + origin as u16 as i32 - i32::from(extent)) as i16,
            _ => origin,
        }
    }
}

pub fn descriptor_origin(descriptor: &ResourceDescriptor, placements: &[ResourcePlacement]) -> Result<PanelPoint, ProducerError> {
    let horizontal = Alignment::parse(descriptor.reserved[2])?;
    let vertical = Alignment::parse(descriptor.layout)?;
    let catalog = descriptor.resource.0 <= 0x83d;
    let measured = descriptor.width != 0 || descriptor.height != 0 || descriptor.reserved[2] != 0 || descriptor.layout != 0;
    let needs_placement = measured || (catalog && (descriptor.x == -20 || descriptor.y == -20));
    let placement = placements.iter().find(|placement| placement.resource == descriptor.resource);
    if needs_placement && placement.is_none() {
        return Err(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(descriptor.resource)));
    }
    let (mut x, mut y) = (descriptor.x, descriptor.y);
    if let Some(placement) = placement {
        x = horizontal.coordinate(x, descriptor.width, placement.extent[0], placement.position[0], catalog);
        y = vertical.coordinate(y, descriptor.height, placement.extent[1], placement.position[1], catalog);
    }
    let x = u16::try_from(x).map_err(|_| ProducerError::Geometry)?;
    let y = u16::try_from(y).map_err(|_| ProducerError::Geometry)?;
    PanelPoint::new(x, y).ok_or(ProducerError::Geometry)
}

pub struct TreeData<'a, 'text> {
    pub node: NodeId,
    pub snapshots: [Option<Snapshot>; 3],
    pub summary: Summary,
    pub rows: &'a [TreeRow],
    pub texts: &'a [TextBinding<'text>],
    pub bitmaps: &'a BitmapCatalog<'a>,
    pub selection: SelectionResolution,
    pub nested: &'a [NestedData<'a, 'text>],
    pub hooks: &'a [ResolvedHook<'a, 'text>],
    pub placements: &'a [ResourcePlacement],
    pub saved_font: FontId,
    pub selected: IndexedItemPalette,
    pub ordinary: IndexedItemPalette,
    pub disabled: IndexedItemPalette,
    pub initial_primary_enabled: bool,
    pub special_mode: SpecialMode,
}

struct CpuTree<'data, 'text, 'storage> {
    data: &'data TreeData<'data, 'text>,
    commands: CommandBuffer<'storage, 'text>,
    font: FontId,
    palette: IndexedItemPalette,
    primary_enabled: bool,
    error: Option<ProducerError>,
}

impl<'data, 'text, 'storage> CpuTree<'data, 'text, 'storage> {
    fn missing(&mut self, prerequisite: SemanticPrerequisite) {
        self.fail(ProducerError::MissingSemantic(prerequisite));
    }
    fn fail(&mut self, error: ProducerError) {
        if self.error.is_none() { self.error = Some(error); }
    }
    fn draw(&mut self, resource: ResourceId, x: i32, y: i32) {
        if self.error.is_some() || resource.0 == 0 { return; }
        let result = (|| {
            let x = u16::try_from(x).map_err(|_| ProducerError::Geometry)?;
            let y = u16::try_from(y).map_err(|_| ProducerError::Geometry)?;
            let origin = PanelPoint::new(x, y).ok_or(ProducerError::Geometry)?;
            let content = match self.data.bitmaps.resolve(resource, Some(self.palette))? {
                ResourceContent::Text(text) => {
                    if self.primary_enabled || self.palette.secondary.alpha.0 != 0 || self.palette.secondary_source != ShapeSecondarySource::White {
                        return Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextFormat));
                    }
                    ResourceContent::Text(self.data.texts.iter().find(|binding| binding.resource == resource).map_or(text, |binding| TextRef::Inline(binding.text)))
                }
                content => content,
            };
            self.commands.resource(origin, content, self.font, self.palette.primary)
        })();
        if let Err(error) = result { self.fail(error); }
    }
    fn row(&self, child: ChildId) -> Option<TreeRow> {
        self.data.rows.iter().find(|row| row.child.id == child.0).copied()
    }
    fn append(&mut self, commands: &[DrawCommand<'text>]) {
        if self.error.is_some() { return; }
        for command in commands {
            if let Err(error) = self.commands.push(*command) { self.fail(error); break; }
        }
    }
    fn hook(&mut self, stage: HookStage, record: &mut HookRecord) -> u32 {
        if let Some(binding) = self.data.hooks.iter().find(|binding| binding.stage == stage && binding.input.0 == record.0 && binding.input_font == self.font && binding.input_palette == self.palette && binding.input_primary_enabled == self.primary_enabled) {
            let commands = binding.commands;
            let output = binding.output;
            let handled = matches!(binding.disposition, HookDisposition::Handled);
            self.append(commands);
            self.font = binding.output_font;
            self.palette = binding.output_palette;
            self.primary_enabled = binding.output_primary_enabled;
            *record = output;
            return u32::from(handled);
        }
        let semantics = if stage == HookStage::Selection {
            self.data.nested.iter().find(|nested| nested.parent == ChildId(record.0[1]))
                .and_then(|nested| nested.rows.iter().find(|row| row.child.selection == record.0[2]))
                .map(|row| row.hook)
        } else { self.row(ChildId(record.0[1])).map(|row| row.hook) };
        match semantics {
            Some(HookSemantics::Absent) => 0,
            _ => { self.missing(SemanticPrerequisite::MenuHook); 1 }
        }
    }
    fn nested_row(&self, parent: ChildId, selection: SelectionId) -> Option<&NestedRow<'data, 'text>> {
        self.data.nested.iter().find(|nested| nested.parent == parent)
            .and_then(|nested| nested.rows.iter().find(|row| row.child.selection == selection.0))
    }
}

impl DrawServices for CpuTree<'_, '_, '_> {
    fn initialize_hook(&mut self) -> HookRecord { HookRecord([0; 16]) }
    fn panel_font(&self) -> FontId { self.data.saved_font }
    fn snapshot(&mut self, node: NodeId, source: SnapshotSource, out: &mut Snapshot) {
        let index = match source { SnapshotSource::Live => 0, SnapshotSource::SavedSlot1 => 1, SnapshotSource::SavedSlot2 => 2 };
        if node != self.data.node { self.missing(SemanticPrerequisite::MenuData); return; }
        match self.data.snapshots[index] { Some(snapshot) => *out = snapshot, None => self.missing(SemanticPrerequisite::MenuData) }
    }
    fn resolve_selection(&mut self, node: NodeId, selection: &mut Selection) -> bool {
        if node != self.data.node { self.missing(SemanticPrerequisite::MenuData); return false; }
        match self.data.selection {
            SelectionResolution::FromRows => false,
            SelectionResolution::Resolved(value) => { *selection = value; true }
            SelectionResolution::Unresolved => { self.missing(SemanticPrerequisite::MenuData); false }
        }
    }
    fn nested_summary(&mut self, node: NodeId, parent: u32, out: &mut Summary) {
        match self.data.nested.iter().find(|nested| node == self.data.node && nested.parent == ChildId(parent) && nested.summary.rows as usize == nested.rows.len()) {
            Some(nested) => *out = nested.summary,
            None => self.missing(SemanticPrerequisite::MenuData),
        }
    }
    fn nested_child(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild) {
        match self.data.nested.iter().find(|nested| node == self.data.node && nested.parent == ChildId(parent)).and_then(|nested| nested.rows.get(index as usize)) {
            Some(row) => *out = row.child,
            None => self.missing(SemanticPrerequisite::MenuData),
        }
    }
    fn nested_label(&mut self, node: NodeId, parent: u32, selection: u32) -> ResourceId {
        if node == self.data.node {
            if let Some(row) = self.nested_row(ChildId(parent), SelectionId(selection)) { return row.label; }
        }
        self.missing(SemanticPrerequisite::MenuData); ResourceId(0)
    }
    fn selection_hook(&mut self, record: &mut HookRecord) -> u32 { self.hook(HookStage::Selection, record) }
    fn selected_detail(&mut self, node: NodeId, parent: u32, selection: u32) {
        match self.nested_row(ChildId(parent), SelectionId(selection)).filter(|_| node == self.data.node).and_then(|row| row.detail) {
            Some(commands) => self.append(commands),
            None => self.missing(SemanticPrerequisite::Footer),
        }
    }
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32) {
        match self.nested_row(ChildId(parent), SelectionId(selection)).filter(|_| node == self.data.node).and_then(|row| row.footer) {
            Some(commands) => self.append(commands),
            None => self.missing(SemanticPrerequisite::Footer),
        }
    }
    fn clear_report(&mut self, out: &mut DrawReport) { *out = DrawReport::zeroed(); }
    fn summarize(&mut self, node: NodeId, out: &mut Summary) {
        if node == self.data.node && self.data.summary.rows as usize == self.data.rows.len() { *out = self.data.summary; }
        else { self.missing(SemanticPrerequisite::MenuData); }
    }
    fn describe(&mut self, _: NodeId, index: u32, out: &mut Child) {
        if let Some(row) = self.data.rows.get(index as usize) { *out = row.child; }
        else { self.missing(SemanticPrerequisite::MenuData); }
    }
    fn divide(&mut self, numerator: u32, denominator: u32) -> u32 {
        match numerator.checked_div(denominator) { Some(value) => value, None => { self.missing(SemanticPrerequisite::MenuData); 0 } }
    }
    fn remainder(&mut self, numerator: u32, denominator: u32) -> u32 {
        match numerator.checked_rem(denominator) { Some(value) => value, None => { self.missing(SemanticPrerequisite::MenuData); 0 } }
    }
    fn label(&mut self, _: NodeId, index: u32) -> ResourceId {
        match self.data.rows.get(index as usize) { Some(row) => row.label, None => { self.missing(SemanticPrerequisite::MenuData); ResourceId(0) } }
    }
    fn font(&mut self, font: FontId) { self.font = font; }
    fn side_state(&mut self, state: SideState) {
        if state.kind != 3 { self.missing(SemanticPrerequisite::TextFormat); }
    }
    fn selection_palette(&mut self, selected: bool, state: u32) {
        self.palette = if selected { self.data.selected } else if state == 2 { self.data.disabled } else { self.data.ordinary };
    }
    fn disable_format(&mut self) { self.primary_enabled = false; }
    fn enable_format(&mut self) { self.primary_enabled = true; }
    fn cell_box(&mut self, _: i32, _: i32, _: i32, _: i32, _: u32, _: u32, _: u32) { self.missing(SemanticPrerequisite::TextFormat); }
    fn label_hook(&mut self, record: &mut HookRecord) -> u32 { self.hook(HookStage::Label, record) }
    fn resource(&mut self, descriptor: &mut ResourceDescriptor) {
        if descriptor.resource.0 == 0 { return; }
        if descriptor.reserved[0] != 0 || descriptor.reserved[1] != 0 || descriptor.tail != 0 {
            self.missing(SemanticPrerequisite::TextFormat); return;
        }
        match descriptor_origin(descriptor, self.data.placements) {
            Ok(origin) => self.draw(descriptor.resource, i32::from(origin.x()), i32::from(origin.y())),
            Err(error) => self.fail(error),
        }
    }
    fn special_mode(&self) -> u32 {
        match self.data.special_mode { SpecialMode::Ordinary => 0, SpecialMode::Two => 2, SpecialMode::Three => 3, SpecialMode::Thirteen => 13 }
    }
    fn text_format(&mut self, enabled: bool) { self.primary_enabled = enabled; }
    fn descriptor_highlight(&mut self, _: u32) { self.missing(SemanticPrerequisite::TextFormat); }
    fn grid_resource(&mut self, first: u32, second: i32, resource: ResourceId) {
        if resource.0 == 0 { return; }
        match stock_grid_origin(first as i32, second, resource) {
            Ok(origin) => self.draw(resource, i32::from(origin.x()), i32::from(origin.y())),
            Err(error) => self.fail(error),
        }
    }
    fn palette_override(&mut self, _: NodeId, child: u32) -> u32 {
        match self.row(ChildId(child)) { Some(row) => u32::from(row.palette_override), None => { self.missing(SemanticPrerequisite::MenuData); 0 } }
    }
    fn primary_palette(&mut self, palette: u32) { self.palette.primary.palette = PaletteIndex(palette); }
    fn value(&mut self, _: NodeId, child: u32, _: u32) -> ResourceId {
        match self.row(ChildId(child)) { Some(row) => row.value, None => { self.missing(SemanticPrerequisite::MenuData); ResourceId(0) } }
    }
    fn value_hook(&mut self, record: &mut HookRecord) -> u32 { self.hook(HookStage::Value, record) }
    fn palette_components(&mut self, index: u32, first: u32, second: u32, third: u32) {
        if first > 255 || third > 255 { self.missing(SemanticPrerequisite::PaletteOptions); return; }
        self.palette.primary = Colour { palette: PaletteIndex(index), alpha: Alpha(first as u8) };
        self.palette.secondary = Colour { palette: PaletteIndex(second), alpha: Alpha(third as u8) };
    }
    fn grid_rectangle(&mut self, _: i32) { self.missing(SemanticPrerequisite::TextFormat); }
}

pub fn compose_tree_and_publish<'text>(
    data: &TreeData<'_, 'text>,
    args: DrawArgs,
    report: &mut DrawReport,
    commands: &mut [DrawCommand<'text>],
    assets: &mut RasterResources<'_, '_, '_>,
    actor: &mut TaskActor,
) -> Result<OutputBatch, FrameError> {
    let mut services = CpuTree { data, commands: CommandBuffer::new(commands), font: data.saved_font, palette: data.ordinary, primary_enabled: data.initial_primary_enabled, error: None };
    let mut next_report = DrawReport::zeroed();
    tree_list::tree_list(&mut services, args, &mut next_report)?;
    if let Some(error) = services.error { return Err(FrameError::Producer(error)); }
    let batch = queue_panel(actor, assets, services.commands.draw_list())?;
    *report = next_report;
    Ok(batch)
}
