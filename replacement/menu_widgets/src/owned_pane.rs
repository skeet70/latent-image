use core::cell::Cell;
use crate::render_commands::commands::{Alpha, Colour, DrawCommand, ShapeSecondarySource, TextRef};
use crate::render_commands::context::OutputBatch;
use crate::render_commands::actor::TaskActor;
use crate::render_commands::producer::{stock_grid_origin, CommandBuffer, ProducerError, ResourceContent, SemanticPrerequisite};
use crate::render_commands::raster::RasterResources;
use crate::render_commands::resources::{BitmapCatalog, IndexedItemPalette, TextBinding};
use crate::render_commands::{FontId, NodeId, PaletteIndex, ResourceId};
use super::owned_frame::{queue_panel, FrameError};
use super::owned_tree::{ChildId, HookSemantics, SelectionId};
use super::owned_picker::{GridPaletteId, GridPaletteTable};
use super::pane_drawer::{self, DrawMode, DrawServices, Format, Hook, HookRecord, NestedChild, Palette, PaneState, RootChild, TabTable};

pub struct PaneRoot {
    pub child: RootChild,
    pub labels: [ResourceId; 2],
    pub value: ResourceId,
    pub special: bool,
    pub label_hook: HookSemantics,
    pub value_hook: HookSemantics,
}

pub struct PaneNested {
    pub parent: ChildId,
    pub child: NestedChild,
    pub value: ResourceId,
    pub hook: HookSemantics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneStyle {
    pub font: FontId,
    pub palette: IndexedItemPalette,
    pub primary_enabled: bool,
}

pub struct PanePalette {
    pub nested: bool,
    pub selected: bool,
    pub state: u32,
    pub palette: IndexedItemPalette,
    pub primary_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneEffect {
    CellBox { x: i32, y: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32 },
    Aligned { x: i32, y: i32, bottom: i32, alignment: u32, resource: ResourceId },
    LabelGrid { x: i32, y: i32, resource: ResourceId },
    Grid { x: i32, y: i32, resource: ResourceId },
    Detail { node: NodeId, parent: ChildId, selection: SelectionId },
    Footer { node: NodeId, parent: ChildId, selection: SelectionId },
    Bounds { x: i32, y: i32, right: i32, bottom: i32 },
    PixelBox { x: i32, y: i32, right: i32, bottom: i32 },
    FinalText,
}

pub struct PaneEffectBinding<'a, 'text> {
    pub effect: PaneEffect,
    pub input_style: PaneStyle,
    pub output_style: PaneStyle,
    pub commands: &'a [DrawCommand<'text>],
}

pub struct PaneHook<'a, 'text> {
    pub kind: Hook,
    pub input: [u32; 13],
    pub output: [u32; 13],
    pub input_style: PaneStyle,
    pub output_style: PaneStyle,
    pub handled: bool,
    pub commands: &'a [DrawCommand<'text>],
}

pub struct PaneTab {
    pub table: TabTable,
    pub bank: u32,
    pub index: u32,
    pub word: u32,
    pub resource: ResourceId,
}

pub struct PaneData<'a, 'text> {
    pub node: NodeId,
    pub mode: DrawMode,
    pub root: NodeId,
    pub state: PaneState,
    pub roots: &'a [PaneRoot],
    pub nested: &'a [PaneNested],
    pub palettes: &'a [PanePalette],
    pub hooks: &'a [PaneHook<'a, 'text>],
    pub effects: &'a [PaneEffectBinding<'a, 'text>],
    pub grid_palettes: Option<&'a GridPaletteTable<'a>>,
    pub tabs: &'a [PaneTab],
    pub texts: &'a [TextBinding<'text>],
    pub bitmaps: &'a BitmapCatalog<'a>,
    pub saved_font: FontId,
    pub initial_style: PaneStyle,
    pub pixel_display: bool,
    pub wide_display: bool,
    pub title_suppressed: bool,
    pub title: ResourceId,
    pub tab_getters: [u32; 4],
    pub markers: [ResourceId; 2],
    pub remembered_colour: u32,
}

struct CpuPane<'data, 'text, 'storage> {
    data: &'data PaneData<'data, 'text>,
    commands: CommandBuffer<'storage, 'text>,
    style: PaneStyle,
    error: Cell<Option<ProducerError>>,
}

impl<'data, 'text, 'storage> CpuPane<'data, 'text, 'storage> {
    fn fail(&self, error: ProducerError) { if self.error.get().is_none() { self.error.set(Some(error)); } }
    fn missing(&self, prerequisite: SemanticPrerequisite) { self.fail(ProducerError::MissingSemantic(prerequisite)); }
    fn append(&mut self, commands: &[DrawCommand<'text>]) {
        if self.error.get().is_some() { return; }
        for command in commands {
            if let Err(error) = self.commands.push(*command) { self.fail(error); break; }
        }
    }
    fn effect(&mut self, effect: PaneEffect) -> bool {
        match self.data.effects.iter().find(|binding| binding.effect == effect && binding.input_style == self.style) {
            Some(binding) => { self.append(binding.commands); self.style = binding.output_style; true }
            None => false,
        }
    }
    fn require_effect(&mut self, effect: PaneEffect, prerequisite: SemanticPrerequisite) {
        if !self.effect(effect) { self.missing(prerequisite); }
    }
    fn select_palette(&mut self, nested: bool, selected: bool, state: u32) {
        match self.data.palettes.iter().find(|palette| palette.nested == nested && palette.selected == selected && palette.state == state) {
            Some(palette) => { self.style.palette = palette.palette; self.style.primary_enabled = palette.primary_enabled; }
            None => self.missing(SemanticPrerequisite::PaletteOptions),
        }
    }
    fn draw(&mut self, x: i32, y: i32, resource: ResourceId) {
        if resource.0 == 0 || self.error.get().is_some() { return; }
        let result = (|| {
            let origin = stock_grid_origin(x, y, resource)?;
            let content = match self.data.bitmaps.resolve(resource, Some(self.style.palette))? {
                ResourceContent::Text(text) => {
                    if self.style.primary_enabled || self.style.palette.secondary.alpha.0 != 0 || self.style.palette.secondary_source != ShapeSecondarySource::White {
                        return Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextFormat));
                    }
                    ResourceContent::Text(self.data.texts.iter().find(|binding| binding.resource == resource).map_or(text, |binding| TextRef::Inline(binding.text)))
                }
                content => content,
            };
            self.commands.resource(origin, content, self.style.font, self.style.palette.primary)
        })();
        if let Err(error) = result { self.fail(error); }
    }
}

impl DrawServices for CpuPane<'_, '_, '_> {
    fn prepare(&mut self, node: NodeId, mode: DrawMode) -> PaneState {
        if node != self.data.node || mode.0 != self.data.mode.0 { self.missing(SemanticPrerequisite::MenuData); }
        self.data.state
    }
    fn panel_font(&self) -> FontId { self.data.saved_font }
    fn font(&mut self, font: FontId) { self.style.font = font; }
    fn describe_root(&mut self, node: NodeId, index: u32, out: &mut RootChild) {
        match self.data.roots.get(index as usize).filter(|_| node == self.data.root) {
            Some(row) => *out = row.child, None => self.missing(SemanticPrerequisite::MenuData),
        }
    }
    fn describe_nested(&mut self, node: NodeId, parent: u32, index: u32, out: &mut NestedChild) {
        match self.data.nested.iter().filter(|row| node == self.data.root && row.parent == ChildId(parent)).nth(index as usize) {
            Some(row) => *out = row.child, None => self.missing(SemanticPrerequisite::MenuData),
        }
    }
    fn label(&mut self, node: NodeId, index: u32, alternate: bool) -> ResourceId {
        match self.data.roots.get(index as usize).filter(|_| node == self.data.root) {
            Some(row) => row.labels[usize::from(alternate)], None => { self.missing(SemanticPrerequisite::MenuData); ResourceId(0) }
        }
    }
    fn special_format(&mut self, node: NodeId, child: u32) -> bool {
        match self.data.roots.iter().find(|row| node == self.data.root && row.child.child == child) {
            Some(row) => row.special, None => { self.missing(SemanticPrerequisite::MenuData); false }
        }
    }
    fn label_style(&mut self, selected: bool, state: u32) { self.select_palette(false, selected, state); }
    fn nested_style(&mut self, selected: bool, state: u32) { self.select_palette(true, selected, state); }
    fn value(&mut self, node: NodeId, child: u32, variant: u32, nested: bool) -> ResourceId {
        let value = if nested {
            self.data.nested.iter().find(|row| node == self.data.root && row.parent == ChildId(child) && row.child.selection == variant).map(|row| row.value)
        } else {
            self.data.roots.iter().find(|row| node == self.data.root && row.child.child == child && row.child.variant == variant).map(|row| row.value)
        };
        match value { Some(value) => value, None => { self.missing(SemanticPrerequisite::MenuData); ResourceId(0) } }
    }
    fn hook(&mut self, kind: Hook, record: &mut HookRecord) -> u32 {
        if let Some(binding) = self.data.hooks.iter().find(|binding| binding.kind == kind && binding.input == record.0 && binding.input_style == self.style) {
            self.append(binding.commands); self.style = binding.output_style; record.0 = binding.output;
            return u32::from(binding.handled);
        }
        let semantics = if kind == Hook::Nested {
            self.data.nested.iter().find(|row| row.parent == ChildId(record.0[1]) && row.child.selection == record.0[2]).map(|row| row.hook)
        } else {
            self.data.roots.iter().find(|row| row.child.child == record.0[1]).map(|row| if kind == Hook::Label { row.label_hook } else { row.value_hook })
        };
        match semantics { Some(HookSemantics::Absent) => 0, _ => { self.missing(SemanticPrerequisite::MenuHook); 1 } }
    }
    fn format(&mut self, format: Format) {
        match format {
            Format::Ia(value) | Format::Text(value) => self.style.primary_enabled = value,
            Format::Brackets(value) => self.style.palette.secondary_source = if value { ShapeSecondarySource::Red } else { ShapeSecondarySource::White },
        }
    }
    fn palette(&mut self, palette: Palette) {
        match palette {
            Palette::Primary(index) => self.style.palette.primary.palette = PaletteIndex(index),
            Palette::Grid(index) => {
                let Some(id) = GridPaletteId::new(index) else { return; };
                match self.data.grid_palettes {
                    Some(table) => {
                        let palette = table.palette(id);
                        self.palette(Palette::Components(palette.index, palette.alpha, palette.component2, palette.component3));
                    }
                    None => self.missing(SemanticPrerequisite::PaletteOptions),
                }
            }
            Palette::Components(index, alpha, second, third) => {
                if alpha > 255 || third > 255 { self.missing(SemanticPrerequisite::PaletteOptions); return; }
                self.style.palette.primary = Colour { palette: PaletteIndex(index), alpha: Alpha(alpha as u8) };
                self.style.palette.secondary = Colour { palette: PaletteIndex(second), alpha: Alpha(third as u8) };
            }
        }
    }
    fn cell_box(&mut self, x: i32, y: i32, right: i32, bottom: i32, stroke: u32, colour: u32, alpha: u32) { self.require_effect(PaneEffect::CellBox { x, y, right, bottom, stroke, colour, alpha }, SemanticPrerequisite::TextFormat); }
    fn aligned(&mut self, x: i32, y: i32, bottom: i32, alignment: u32, resource: ResourceId) {
        if resource.0 != 0 { self.require_effect(PaneEffect::Aligned { x, y, bottom, alignment, resource }, SemanticPrerequisite::Alignment); }
    }
    fn label_grid(&mut self, x: i32, y: i32, resource: ResourceId) {
        if resource.0 != 0 && !self.effect(PaneEffect::LabelGrid { x, y, resource }) { self.draw(x, y, resource); }
    }
    fn grid(&mut self, x: i32, y: i32, resource: ResourceId) {
        if resource.0 != 0 && !self.effect(PaneEffect::Grid { x, y, resource }) { self.draw(x, y, resource); }
    }
    fn detail(&mut self, node: NodeId, parent: u32, selection: u32) { self.require_effect(PaneEffect::Detail { node, parent: ChildId(parent), selection: SelectionId(selection) }, SemanticPrerequisite::Footer); }
    fn footer(&mut self, node: NodeId, parent: u32, selection: u32) { self.require_effect(PaneEffect::Footer { node, parent: ChildId(parent), selection: SelectionId(selection) }, SemanticPrerequisite::Footer); }
    fn pixel_display(&self) -> bool { self.data.pixel_display }
    fn wide_display(&self) -> bool { self.data.wide_display }
    fn bounds(&mut self, x: i32, y: i32, right: i32, bottom: i32) { self.require_effect(PaneEffect::Bounds { x, y, right, bottom }, SemanticPrerequisite::TextFormat); }
    fn pixel_box(&mut self, x: i32, y: i32, right: i32, bottom: i32) { self.require_effect(PaneEffect::PixelBox { x, y, right, bottom }, SemanticPrerequisite::TextFormat); }
    fn title_suppressed(&mut self) -> bool { self.data.title_suppressed }
    fn title_resource(&mut self) -> ResourceId { self.data.title }
    fn tab_getters(&mut self) -> [u32; 4] { self.data.tab_getters }
    fn tab_resource(&self, table: TabTable, bank: u32, index: u32, word: u32) -> ResourceId {
        match self.data.tabs.iter().find(|tab| tab.table == table && tab.bank == bank && tab.index == index && tab.word == word) {
            Some(tab) => tab.resource, None => { self.missing(SemanticPrerequisite::MenuData); ResourceId(0) }
        }
    }
    fn style_marker(&mut self, nested: bool) -> ResourceId { self.data.markers[usize::from(nested)] }
    fn remembered_colour(&mut self) -> u32 { self.data.remembered_colour }
    fn final_text(&mut self) { self.require_effect(PaneEffect::FinalText, SemanticPrerequisite::TextFormat); }
}

pub fn compose_pane_and_publish<'text>(data: &PaneData<'_, 'text>, node: NodeId, mode: DrawMode, commands: &mut [DrawCommand<'text>], assets: &mut RasterResources<'_, '_, '_>, actor: &mut TaskActor) -> Result<OutputBatch, FrameError> {
    let mut services = CpuPane { data, commands: CommandBuffer::new(commands), style: data.initial_style, error: Cell::new(None) };
    pane_drawer::pane_drawer(&mut services, node, mode)?;
    if let Some(error) = services.error.get() { return Err(FrameError::Producer(error)); }
    queue_panel(actor, assets, services.commands.draw_list())
}
