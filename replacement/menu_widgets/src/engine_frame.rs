use crate::menu_engine::state::{EngineState, MenuOpen, RowState as EngineRowState};
use crate::menu_engine::tree::{Label, RowChild, Tree};
use crate::render_commands::commands::DrawCommand;
use crate::render_commands::producer::{MenuLayout, MenuStyle, MenuView, ProducerError, RowState, RowView, SemanticPrerequisite};
use crate::render_commands::context::OutputBatch;
use crate::render_commands::actor::TaskActor;
use crate::render_commands::raster::RasterResources;
use crate::render_commands::resources::{BitmapCatalog, IndexedItemPalette};
use crate::render_commands::{PanelRect, ResourceId, RowIndex};
use super::owned_frame::{compose_menu_and_publish, FrameError};
use super::owned_tree::HookSemantics;

#[derive(Clone, Copy)]
pub enum LabelRole { Name, ListIcon, PreviewIcon, Help }

pub enum RowValues<'a> { NotDisplayed, Resolved(&'a [Option<ResourceId>]), Unavailable }

pub fn compose_engine_menu_and_publish<'assets>(
    tree: &Tree,
    state: &EngineState,
    labels: &[Option<&[Label]>],
    bitmaps: &BitmapCatalog<'_>,
    palettes: &[Option<IndexedItemPalette>],
    label_role: LabelRole,
    hooks: &[HookSemantics],
    values: RowValues<'_>,
    row_storage: &mut [RowView<'assets>],
    mut layout: MenuLayout,
    style: MenuStyle,
    commands: &mut [DrawCommand<'assets>],
    assets: &mut RasterResources<'_, '_, '_>,
    actor: &mut TaskActor,
) -> Result<OutputBatch, FrameError> {
    let missing = || FrameError::Producer(ProducerError::MissingSemantic(SemanticPrerequisite::MenuData));
    if state.menu_open != MenuOpen::Open { return Err(missing()); }
    let reference = state.node.ok_or_else(missing)?;
    let node = tree.node(reference).ok_or_else(missing)?;
    let rows = node.rows.ok_or_else(missing)?;
    let labels = labels.get(reference.index()).copied().flatten().ok_or_else(missing)?;
    if rows.len() != labels.len() || rows.len() != hooks.len() || rows.len() != palettes.len() || rows.len() > state.row_states.len() { return Err(missing()); }
    if rows.len() > row_storage.len() { return Err(FrameError::Producer(ProducerError::Capacity)); }
    let values = match values {
        RowValues::NotDisplayed => None,
        RowValues::Resolved(values) if values.len() == rows.len() => Some(values),
        _ => return Err(missing()),
    };
    let capacity = node.rows_per_page.get();
    let height = capacity.checked_mul(u32::from(layout.row_height.get())).and_then(|height| u16::try_from(height).ok()).ok_or(FrameError::Producer(ProducerError::Geometry))?;
    if height > layout.bounds.height() { return Err(FrameError::Producer(ProducerError::Geometry)); }
    layout.bounds = PanelRect::new(layout.bounds.origin(), layout.bounds.width(), height).ok_or(FrameError::Producer(ProducerError::Geometry))?;
    let mut visible = 0u32;
    let mut scroll = None;
    for (index, ((row, label), hook)) in rows.iter().zip(labels).zip(hooks).enumerate() {
        if matches!(row.child, RowChild::Package) { return Err(missing()); }
        if matches!(hook, HookSemantics::Unresolved) { return Err(FrameError::Producer(ProducerError::MissingSemantic(SemanticPrerequisite::MenuHook))); }
        let row_state = match state.row_states[index] {
            EngineRowState::Offered | EngineRowState::Variant => RowState::Enabled,
            EngineRowState::Disabled => RowState::Disabled,
            EngineRowState::Hidden => RowState::Hidden,
        };
        if row_state != RowState::Hidden {
            if visible == state.live.scroll.get() { scroll = Some(RowIndex(index as u32)); }
            visible += 1;
        }
        let label = match label_role { LabelRole::Name => label.name, LabelRole::ListIcon => label.list_icon, LabelRole::PreviewIcon => label.preview_icon, LabelRole::Help => label.help };
        let value = match values.and_then(|values| values[index]) {
            Some(resource) => Some(bitmaps.resolve(resource, palettes[index]).map_err(FrameError::Producer)?),
            None => None,
        };
        row_storage[index] = RowView { label: bitmaps.resolve(ResourceId(label.get()), palettes[index]).map_err(FrameError::Producer)?, value, state: row_state };
    }
    let scroll = if rows.is_empty() { RowIndex(0) } else { scroll.ok_or_else(missing)? };
    let cursor = if rows.is_empty() { None } else { Some(RowIndex(state.live.cursor.get())) };
    compose_menu_and_publish(MenuView { rows: &row_storage[..rows.len()], cursor, scroll_top: scroll, layout, style }, commands, assets, actor)
}
