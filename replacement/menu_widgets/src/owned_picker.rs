use crate::render_commands::commands::{Alpha, Colour, DrawCommand, ShapeSecondarySource, TextRef};
use crate::render_commands::context::OutputBatch;
use crate::render_commands::actor::TaskActor;
use crate::render_commands::producer::{stock_grid_origin, CommandBuffer, ProducerError, ResourceContent, SemanticPrerequisite};
use crate::render_commands::raster::{AssetProvenance, RasterResources};
use crate::render_commands::resources::{BitmapCatalog, IndexedItemPalette, TextBinding};
use crate::render_commands::{DrawReport, FontId, NodeId, PaletteIndex, ResourceId, RowIndex};
use super::owned_frame::{queue_panel, FrameError};
use super::owned_tree::{descriptor_origin, ChildId, HookSemantics, ResourcePlacement, SelectionId};
use super::tree_list::ResourceDescriptor;
use super::value_picker::{self, CellFill, Child, DrawArgs, GridFill, HookRecord, LabelField, Palette, PickerServices, PixelFill, ResourceRecord, Snapshot, SnapshotSelection, Summary};

pub struct PickerRow {
    pub child: Child,
    pub labels: [ResourceId; 4],
    pub hook: HookSemantics,
}

pub struct PickerMetrics {
    pub resource: ResourceId,
    pub columns: [u32; 2],
    pub size: [u8; 2],
    pub extent: [u8; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridPaletteId(u8);
impl GridPaletteId {
    pub const fn new(raw: u32) -> Option<Self> { if raw <= 120 { Some(Self(raw as u8)) } else { None } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridPaletteError { Length, Provenance }

pub struct GridPaletteTable<'a> {
    bytes: &'a [u8; 121 * 16],
    provenance: AssetProvenance<'a>,
}
impl<'a> GridPaletteTable<'a> {
    pub fn parse(bytes: &'a [u8], provenance: AssetProvenance<'a>) -> Result<Self, GridPaletteError> {
        let valid = match provenance {
            AssetProvenance::Original { author } => !author.trim().is_empty(),
            AssetProvenance::OpenLicensed { license, attribution } => !license.trim().is_empty() && !attribution.trim().is_empty(),
            AssetProvenance::PrivateExtract { source } => !source.trim().is_empty(),
        };
        if !valid { return Err(GridPaletteError::Provenance); }
        Ok(Self { bytes: bytes.try_into().map_err(|_| GridPaletteError::Length)?, provenance })
    }
    pub const fn provenance(&self) -> AssetProvenance<'a> { self.provenance }
    pub fn palette(&self, id: GridPaletteId) -> Palette {
        let start = usize::from(id.0) * 16;
        let word = |offset| u32::from_le_bytes(self.bytes[start + offset..start + offset + 4].try_into().unwrap());
        Palette { index: word(0), alpha: word(4), component2: word(8), component3: word(12) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerEffect {
    Resource(ResourceRecord), GridResource { x: i32, y: i32, resource: ResourceId },
    GridFill(GridFill), CellFill(CellFill), PixelFill(PixelFill),
    MetricFooter(u8), ResolutionFooter { node: NodeId, parent: ChildId, child: SelectionId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PickerStyle {
    pub font: FontId,
    pub palette: Palette,
    pub angle: bool,
    pub primary_enabled: bool,
}

pub struct PickerEffectBinding<'a, 'text> {
    pub effect: PickerEffect,
    pub input_style: PickerStyle,
    pub output_style: PickerStyle,
    pub commands: &'a [DrawCommand<'text>],
}

pub struct PickerHook<'a, 'text> {
    pub input: HookRecord,
    pub output: HookRecord,
    pub input_style: PickerStyle,
    pub output_style: PickerStyle,
    pub handled: bool,
    pub commands: &'a [DrawCommand<'text>],
}

pub struct PickerData<'a, 'text> {
    pub node: NodeId,
    pub snapshots: [Option<Snapshot>; 2],
    pub parent: ChildId,
    pub summary: Summary,
    pub rows: &'a [PickerRow],
    pub metrics: &'a [PickerMetrics],
    pub placements: &'a [ResourcePlacement],
    pub texts: &'a [TextBinding<'text>],
    pub bitmaps: &'a BitmapCatalog<'a>,
    pub hooks: &'a [PickerHook<'a, 'text>],
    pub effects: &'a [PickerEffectBinding<'a, 'text>],
    pub grid_palettes: Option<&'a GridPaletteTable<'a>>,
    pub side_state: [u32; 5],
    pub saved_font: FontId,
    pub initial_style: PickerStyle,
    pub palette_component: u8,
    pub bar_style: u32,
    pub bar_resources: [ResourceId; 3],
    pub menu_mode: u32,
    pub help_hidden: bool,
}

struct CpuPicker<'data, 'text, 'storage> {
    data: &'data PickerData<'data, 'text>,
    commands: CommandBuffer<'storage, 'text>,
    style: PickerStyle,
    error: Option<ProducerError>,
}

impl<'data, 'text, 'storage> CpuPicker<'data, 'text, 'storage> {
    fn fail(&mut self, error: ProducerError) { if self.error.is_none() { self.error = Some(error); } }
    fn missing(&mut self, prerequisite: SemanticPrerequisite) { self.fail(ProducerError::MissingSemantic(prerequisite)); }
    fn append(&mut self, commands: &[DrawCommand<'text>]) {
        if self.error.is_some() { return; }
        for command in commands {
            if let Err(error) = self.commands.push(*command) { self.fail(error); break; }
        }
    }
    fn effect(&mut self, effect: PickerEffect) -> bool {
        match self.data.effects.iter().find(|binding| binding.effect == effect && binding.input_style == self.style) {
            Some(binding) => { self.append(binding.commands); self.style = binding.output_style; true }
            None => false,
        }
    }
    fn require_effect(&mut self, effect: PickerEffect, prerequisite: SemanticPrerequisite) {
        if !self.effect(effect) { self.missing(prerequisite); }
    }
    fn matches(&mut self, node: NodeId, parent: ChildId) -> bool {
        let matches = node == self.data.node && parent == self.data.parent;
        if !matches { self.missing(SemanticPrerequisite::MenuData); }
        matches
    }
    fn metrics(&mut self, resource: ResourceId) -> Option<&'data PickerMetrics> {
        let metrics = self.data.metrics.iter().find(|metrics| metrics.resource == resource);
        if metrics.is_none() { self.missing(SemanticPrerequisite::ResourceMetrics(resource)); }
        metrics
    }
    fn draw(&mut self, resource: ResourceId, origin: crate::render_commands::PanelPoint) {
        if resource.0 == 0 || self.error.is_some() { return; }
        let palette = self.style.palette;
        if palette.alpha > 255 || palette.component3 > 255 { self.missing(SemanticPrerequisite::PaletteOptions); return; }
        let snapshot = IndexedItemPalette {
            primary: Colour { palette: PaletteIndex(palette.index), alpha: Alpha(palette.alpha as u8) },
            secondary: Colour { palette: PaletteIndex(palette.component2), alpha: Alpha(palette.component3 as u8) },
            secondary_source: if self.style.angle { ShapeSecondarySource::Red } else { ShapeSecondarySource::White },
        };
        let result = (|| {
            let content = match self.data.bitmaps.resolve(resource, Some(snapshot))? {
                ResourceContent::Text(text) => {
                    if self.style.angle || self.style.primary_enabled || snapshot.secondary.alpha.0 != 0 {
                        return Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextFormat));
                    }
                    ResourceContent::Text(self.data.texts.iter().find(|binding| binding.resource == resource).map_or(text, |binding| TextRef::Inline(binding.text)))
                }
                content => content,
            };
            self.commands.resource(origin, content, self.style.font, snapshot.primary)
        })();
        if let Err(error) = result { self.fail(error); }
    }
}

impl PickerServices for CpuPicker<'_, '_, '_> {
    fn empty_hook(&mut self) -> HookRecord { HookRecord { node: 0, parent: 0, child: 0, side_state: [0; 5], x: 0, y: 0, word10: 0, selected: 0, label: 0, reserved: [0; 3] } }
    fn clear_report(&mut self, report: &mut DrawReport) { *report = DrawReport::zeroed(); }
    fn panel_font(&mut self) -> FontId { self.data.saved_font }
    fn snapshot(&mut self, node: NodeId, selection: SnapshotSelection) -> Snapshot {
        let index = match selection { SnapshotSelection::Live => 0, SnapshotSelection::SavedSlotTwo => 1 };
        if node == self.data.node {
            if let Some(snapshot) = self.data.snapshots[index] { return snapshot; }
        }
        self.missing(SemanticPrerequisite::MenuData); Snapshot::default()
    }
    fn summary(&mut self, node: NodeId, parent: u32) -> Summary {
        if self.matches(node, ChildId(parent)) && self.data.summary.children as usize == self.data.rows.len() { self.data.summary }
        else { self.missing(SemanticPrerequisite::MenuData); Summary::default() }
    }
    fn child(&mut self, node: NodeId, parent: u32, index: RowIndex) -> Child {
        if self.matches(node, ChildId(parent)) {
            if let Some(row) = self.data.rows.get(index.0 as usize) { return row.child; }
        }
        self.missing(SemanticPrerequisite::MenuData); Child::default()
    }
    fn label(&mut self, node: NodeId, parent: u32, child: u32, field: LabelField) -> ResourceId {
        if self.matches(node, ChildId(parent)) {
            if let Some(row) = self.data.rows.iter().find(|row| row.child.id == child) { return row.labels[field as usize]; }
        }
        self.missing(SemanticPrerequisite::MenuData); ResourceId(0)
    }
    fn label_columns(&mut self, resource: ResourceId, catalog: bool) -> u32 { self.metrics(resource).map_or(0, |metrics| metrics.columns[usize::from(!catalog)]) }
    fn page_quotient(&mut self, numerator: u32, denominator: u32) -> u32 {
        match numerator.checked_div(denominator) { Some(value) => value, None => { self.missing(SemanticPrerequisite::MenuData); 0 } }
    }
    fn side_state(&mut self) -> [u32; 5] { self.data.side_state }
    fn font(&mut self, font: FontId) { self.style.font = font; }
    fn palette(&mut self, palette: Palette) { self.style.palette = palette; }
    fn grid_palette(&mut self, palette: u32) {
        let Some(id) = GridPaletteId::new(palette) else { return; };
        match self.data.grid_palettes {
            Some(table) => self.style.palette = table.palette(id),
            None => self.missing(SemanticPrerequisite::PaletteOptions),
        }
    }
    fn hook(&mut self, record: &mut HookRecord) -> u32 {
        if let Some(binding) = self.data.hooks.iter().find(|binding| binding.input == *record && binding.input_style == self.style) {
            self.append(binding.commands);
            self.style = binding.output_style;
            *record = binding.output;
            return u32::from(binding.handled);
        }
        match self.data.rows.iter().find(|row| row.child.id == record.child).map(|row| row.hook) {
            Some(HookSemantics::Absent) => 0,
            _ => { self.missing(SemanticPrerequisite::MenuHook); 1 }
        }
    }
    fn resource(&mut self, record: &ResourceRecord) {
        if record.label == 0 || self.effect(PickerEffect::Resource(*record)) { return; }
        if record.word3 != 0 || record.word4 != 0 || record.word7 != 0 { self.missing(SemanticPrerequisite::TextFormat); return; }
        let descriptor = ResourceDescriptor { resource: ResourceId(record.label), x: record.x as i16, y: record.y as i16, width: record.width as i16, height: record.height as i16, reserved: [0, 0, record.style], layout: record.flags, tail: 0 };
        match descriptor_origin(&descriptor, self.data.placements) {
            Ok(origin) => self.draw(descriptor.resource, origin),
            Err(error) => self.fail(error),
        }
    }
    fn begin_format(&mut self) { self.style.angle = true; }
    fn end_format(&mut self) { self.style.angle = false; }
    fn palette_component(&mut self) -> u32 { u32::from(self.data.palette_component) }
    fn grid_resource(&mut self, x: i32, y: i32, resource: ResourceId) {
        if resource.0 == 0 || self.effect(PickerEffect::GridResource { x, y, resource }) { return; }
        match stock_grid_origin(x, y, resource) {
            Ok(origin) => self.draw(resource, origin), Err(error) => self.fail(error),
        }
    }
    fn grid_fill(&mut self, fill: GridFill) { self.require_effect(PickerEffect::GridFill(fill), SemanticPrerequisite::TextFormat); }
    fn cell_fill(&mut self, fill: CellFill) { self.require_effect(PickerEffect::CellFill(fill), SemanticPrerequisite::TextFormat); }
    fn pixel_fill(&mut self, fill: PixelFill) { self.require_effect(PickerEffect::PixelFill(fill), SemanticPrerequisite::TextFormat); }
    fn text_format(&mut self, enabled: bool) { self.style.primary_enabled = enabled; }
    fn bar_style(&mut self) -> u32 { self.data.bar_style }
    fn bar_resources(&mut self, style: u32) -> [ResourceId; 3] {
        if style != self.data.bar_style { self.missing(SemanticPrerequisite::MenuData); }
        self.data.bar_resources
    }
    fn grid_size(&mut self, resource: ResourceId) -> [u8; 2] { self.metrics(resource).map_or([0; 2], |metrics| metrics.size) }
    fn grid_extent(&mut self, resource: ResourceId) -> [u8; 2] { self.metrics(resource).map_or([0; 2], |metrics| metrics.extent) }
    fn menu_mode(&mut self) -> u32 { self.data.menu_mode }
    fn setting_help_hidden(&mut self) -> bool { self.data.help_hidden }
    fn metric_footer(&mut self, column: u8) { self.require_effect(PickerEffect::MetricFooter(column), SemanticPrerequisite::Footer); }
    fn resolution_footer(&mut self, node: NodeId, parent: u32, child: u32) { self.require_effect(PickerEffect::ResolutionFooter { node, parent: ChildId(parent), child: SelectionId(child) }, SemanticPrerequisite::Footer); }
}

pub fn compose_picker_and_publish<'text>(data: &PickerData<'_, 'text>, args: DrawArgs, report: &mut DrawReport, commands: &mut [DrawCommand<'text>], assets: &mut RasterResources<'_, '_, '_>, actor: &mut TaskActor) -> Result<Option<OutputBatch>, FrameError> {
    if args.mode.snapshot().is_none() { return Ok(None); }
    let mut services = CpuPicker { data, commands: CommandBuffer::new(commands), style: data.initial_style, error: None };
    let mut next_report = *report;
    value_picker::value_picker(&mut services, args, &mut next_report);
    if let Some(error) = services.error { return Err(FrameError::Producer(error)); }
    let batch = queue_panel(actor, assets, services.commands.draw_list())?;
    *report = next_report;
    Ok(Some(batch))
}
