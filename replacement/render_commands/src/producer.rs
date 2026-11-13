use super::commands::{Alpha, Colour, DrawCommand, DrawList, ShapePaint, TextRef};
use super::{CatalogTextId, CustomTextId, FontId, ImageId, PanelPoint, PanelRect, ResourceId, RowIndex, SurfaceId};
use super::resources::{emission_text, BitmapError, FormatAction, FormatEmission, ResourceError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticPrerequisite {
    CatalogClassification(ResourceId),
    CustomResource(ResourceId),
    PlaneResource(ResourceId),
    ResourceMetrics(ResourceId),
    TextFormat,
    Alignment,
    MenuHook,
    TextCache,
    BitmapKind(ResourceId),
    PaletteOptions,
    MenuData,
    Footer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProducerError {
    MissingSemantic(SemanticPrerequisite),
    Resource(ResourceError),
    Bitmap(BitmapError),
    Capacity,
    Geometry,
    Selection,
    ReportRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceContent<'a> {
    Text(TextRef<'a>),
    Image { image: ImageId, alpha: Alpha },
    Shape { image: ImageId, alpha: Alpha, paint: ShapePaint },
    Unavailable(SemanticPrerequisite),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowState { Enabled, Disabled, Hidden }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowView<'a> {
    pub label: ResourceContent<'a>,
    pub value: Option<ResourceContent<'a>>,
    pub state: RowState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuStyle {
    pub font: FontId,
    pub normal: Colour,
    pub selected: Colour,
    pub disabled: Colour,
    pub selection_background: Colour,
    pub background: Colour,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuLayout {
    pub bounds: PanelRect,
    pub row_height: core::num::NonZeroU16,
    pub label_inset: u16,
    pub value_inset: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuView<'rows, 'assets> {
    pub rows: &'rows [RowView<'assets>],
    pub cursor: Option<RowIndex>,
    pub scroll_top: RowIndex,
    pub layout: MenuLayout,
    pub style: MenuStyle,
}

pub struct CommandBuffer<'storage, 'assets> {
    storage: &'storage mut [DrawCommand<'assets>],
    len: usize,
}

impl<'storage, 'assets> CommandBuffer<'storage, 'assets> {
    pub fn new(storage: &'storage mut [DrawCommand<'assets>]) -> Self {
        Self { storage, len: 0 }
    }

    pub fn draw_list(&self) -> DrawList<'_> {
        DrawList::new(SurfaceId::Osd, &self.storage[..self.len])
    }

    pub fn push(&mut self, command: DrawCommand<'assets>) -> Result<(), ProducerError> {
        let slot = self.storage.get_mut(self.len).ok_or(ProducerError::Capacity)?;
        *slot = command;
        self.len += 1;
        Ok(())
    }

    pub fn resource(
        &mut self,
        origin: PanelPoint,
        resource: ResourceContent<'assets>,
        font: FontId,
        colour: Colour,
    ) -> Result<(), ProducerError> {
        self.push(match resource {
            ResourceContent::Text(text) => DrawCommand::Text { origin, text, font, colour },
            ResourceContent::Image { image, alpha } => DrawCommand::Image { origin, image, alpha },
            ResourceContent::Shape { image, alpha, paint } => DrawCommand::Shape { origin, image, alpha, paint },
            ResourceContent::Unavailable(prerequisite) => return Err(ProducerError::MissingSemantic(prerequisite)),
        })
    }

    pub fn stock_formatted(
        &mut self,
        emissions: &[FormatEmission<'_>],
        text_storage: &'assets mut [[u8; 128]],
        alpha: super::commands::Alpha,
    ) -> Result<(), ProducerError> {
        let start = self.len;
        let result = (|| {
            if emissions.len() > text_storage.len() { return Err(ProducerError::Capacity); }
            for (emission, storage) in emissions.iter().zip(text_storage) {
                let text = match emission.action {
                    FormatAction::Pen(operation) => { self.push(DrawCommand::TextPen(operation))?; continue; }
                    FormatAction::Text(text) => text,
                };
                let state = emission.state;
                if state.options.angle || state.options.parenthesis || state.options.primary_enabled
                    || state.scale.iter().any(|scale| scale.0 != 0 && scale.0 != 100)
                    || state.offset != [0; 2] || state.packed_bounds != 0 || state.bounds != [0; 2] {
                    return Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextFormat));
                }
                let text = emission_text(text, storage).map_err(ProducerError::Resource)?;
                let opacity = ((u16::from(alpha.0) * u16::from(state.primary_alpha.0) + 127) / 255) as u8;
                self.push(DrawCommand::ContinueText { text: TextRef::Inline(text), font: state.font, colour: Colour { palette: state.primary, alpha: super::commands::Alpha(opacity) } })?;
            }
            Ok(())
        })();
        if result.is_err() { self.len = start; }
        result
    }

    pub fn compose_menu(&mut self, view: MenuView<'_, 'assets>) -> Result<(), ProducerError> {
        let start = self.len;
        let result = self.append_menu(view);
        if result.is_err() { self.len = start; }
        result
    }

    fn append_menu(&mut self, view: MenuView<'_, 'assets>) -> Result<(), ProducerError> {
        if let Some(cursor) = view.cursor {
            let row = view.rows.get(cursor.0 as usize).ok_or(ProducerError::Selection)?;
            if row.state != RowState::Enabled { return Err(ProducerError::Selection); }
        }
        if view.scroll_top.0 as usize > view.rows.len() { return Err(ProducerError::Selection); }
        let layout = view.layout;
        if layout.label_inset >= layout.bounds.width() || layout.value_inset >= layout.bounds.width() {
            return Err(ProducerError::Geometry);
        }
        let panel = PanelRect::new(PanelPoint::new(0, 0).ok_or(ProducerError::Geometry)?, 720, 480).ok_or(ProducerError::Geometry)?;
        self.push(DrawCommand::ResetClip)?;
        self.push(DrawCommand::Fill { rect: panel, colour: view.style.background })?;
        self.push(DrawCommand::Clip(layout.bounds))?;
        let mut visible = 0u16;
        for (index, row) in view.rows.iter().enumerate().skip(view.scroll_top.0 as usize) {
            if row.state == RowState::Hidden { continue; }
            let offset = u32::from(visible) * u32::from(layout.row_height.get());
            if offset + u32::from(layout.row_height.get()) > u32::from(layout.bounds.height()) { break; }
            let y = layout.bounds.origin().y() + offset as u16;
            let selected = view.cursor == Some(RowIndex(index as u32));
            let colour = if row.state == RowState::Disabled { view.style.disabled }
                else if selected { view.style.selected } else { view.style.normal };
            if selected {
                let origin = PanelPoint::new(layout.bounds.origin().x(), y).ok_or(ProducerError::Geometry)?;
                let rect = PanelRect::new(origin, layout.bounds.width(), layout.row_height.get()).ok_or(ProducerError::Geometry)?;
                self.push(DrawCommand::Fill { rect, colour: view.style.selection_background })?;
            }
            let label = PanelPoint::new(layout.bounds.origin().x() + layout.label_inset, y).ok_or(ProducerError::Geometry)?;
            self.resource(label, row.label, view.style.font, colour)?;
            if let Some(value) = row.value {
                let origin = PanelPoint::new(layout.bounds.origin().x() + layout.value_inset, y).ok_or(ProducerError::Geometry)?;
                self.resource(origin, value, view.style.font, colour)?;
            }
            visible += 1;
        }
        self.push(DrawCommand::ResetClip)
    }
}

pub fn stock_grid_origin(column: i32, row: i32, resource: ResourceId) -> Result<PanelPoint, ProducerError> {
    if column == -20 || row == -20 {
        return Err(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(resource)));
    }
    let x = column.checked_sub(1).and_then(|n| n.checked_mul(20)).and_then(|n| u16::try_from(n).ok());
    let y = row.checked_sub(1).and_then(|n| n.checked_mul(24)).and_then(|n| u16::try_from(n).ok());
    match (x, y) {
        (Some(x), Some(y)) => PanelPoint::new(x, y).ok_or(ProducerError::Geometry),
        _ => Err(ProducerError::Geometry),
    }
}

pub fn classify_stock_resource(resource: ResourceId) -> Result<ResourceContent<'static>, ProducerError> {
    match resource.0 {
        0..=0x83d => CatalogTextId::new(resource.0).map(|id| ResourceContent::Text(TextRef::Catalog(id))).ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::CatalogClassification(resource))),
        0xfe000000 => Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextCache)),
        0xff000000..=0xff0008c5 => Err(ProducerError::MissingSemantic(SemanticPrerequisite::BitmapKind(resource))),
        0xfd000000..=0xfd000001 => CustomTextId::new(resource.0 - 0xfd000000).map(|id| ResourceContent::Text(TextRef::Custom(id))).ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::CustomResource(resource))),
        0xfc000000..=0xfcffffff => Err(ProducerError::MissingSemantic(SemanticPrerequisite::PlaneResource(resource))),
        _ => Err(ProducerError::MissingSemantic(SemanticPrerequisite::CatalogClassification(resource))),
    }
}

pub struct CatalogMetrics<'a> {
    pub id: CatalogTextId,
    pub dimensions: [u16; 2],
    pub metrics: Option<&'a [i16]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricMode { Enabled, Disabled }

pub fn catalog_dimensions(resource: ResourceId, catalog: &[CatalogMetrics<'_>]) -> Result<[u16; 2], ProducerError> {
    if resource.0 == 0 || resource.0 > 0x83d { return Ok([0; 2]); }
    catalog.iter().find(|record| record.id.value() == resource.0).map(|record| record.dimensions)
        .ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(resource)))
}

pub fn catalog_metric(resource: ResourceId, index: usize, mode: MetricMode, catalog: &[CatalogMetrics<'_>]) -> Result<i16, ProducerError> {
    if mode == MetricMode::Disabled || resource.0 >= 0x83c { return Ok(0); }
    let record = catalog.iter().find(|record| record.id.value() == resource.0)
        .ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(resource)))?;
    match record.metrics {
        None => Ok(0),
        Some(metrics) => metrics.get(index).copied().ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(resource))),
    }
}
