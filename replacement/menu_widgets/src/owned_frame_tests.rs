extern crate std;

use super::owned_frame::{compose_menu_and_publish, FrameError};
use crate::render_commands::commands::{Alpha, Colour, DrawCommand, ShapeSecondarySource};
use crate::render_commands::producer::{MenuLayout, MenuStyle, MenuView, ProducerError, ResourceContent, RowState, RowView, SemanticPrerequisite};
use crate::render_commands::publisher::{ContextId, Publisher, SubmissionError};
use crate::render_commands::raster::{AssetPack, AssetProvenance, RasterError, RasterResources};
use crate::render_commands::context::GraphicsContext;
use crate::render_commands::actor::{ActorError, TaskActor};
use crate::render_commands::actor_request::{InitializationWord, QueueRequest};
use crate::render_commands::runtime::RenderError;
use crate::render_commands::resources::{BitmapCatalog, DataAddress, DataRegion, IndexedItemPalette, ResourceMemory};
use crate::render_commands::{CatalogTextId, CustomTextId, FontId, ImageId, PaletteIndex, PanelPoint, PanelRect, RowIndex, SurfaceId};
use crate::render_commands::test_buffers::StaticBuffers;
use std::vec::Vec;

fn panel_context(buffers: &'static StaticBuffers, identity: u64) -> GraphicsContext {
    let [first, second] = buffers.take();
    let publisher = Publisher::new(ContextId::new(identity).unwrap(), SurfaceId::Osd, first, second).unwrap();
    let mut context = GraphicsContext::new();
    assert!(context.install(publisher).is_ok());
    context
}

fn panel_actor(buffers: &'static StaticBuffers, identity: u64) -> TaskActor {
    let mut actor = TaskActor::new();
    assert!(actor.open(panel_context(buffers, identity), QueueRequest::Open { initialization: InitializationWord(0), waiter: None }).is_ok());
    actor
}

fn format_text(emission: crate::render_commands::resources::FormatEmission<'_>) -> crate::render_commands::resources::EmittedText<'_> {
    match emission.action {
        crate::render_commands::resources::FormatAction::Text(text) => text,
        crate::render_commands::resources::FormatAction::Pen(_) => panic!("expected text emission"),
    }
}

fn record(bytes: &mut Vec<u8>, kind: u8, id: u32, dimensions: [u16; 3], payload: &[u8]) {
    bytes.extend_from_slice(&[kind, 0, 0, 0]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for value in dimensions { bytes.extend_from_slice(&value.to_le_bytes()); }
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn assets() -> Vec<u8> {
    let mut bytes = b"RCP1".to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    record(&mut bytes, 1, 0, [0; 3], &[0, 0, 0, 255]);
    record(&mut bytes, 1, 1, [0; 3], &[0, 0, 255, 255]);
    record(&mut bytes, 2, 7, [1, 1, 4], &[255, 0, 0, 255]);
    bytes
}

fn bitmap_table() -> Vec<u8> {
    let mut bytes = std::vec![0; 244 * 12];
    for image in [7usize, 0xe7, 0xf3] {
        bytes[image * 12..image * 12 + 4].copy_from_slice(&[1, 0, 1, 0]);
    }
    bytes
}

fn fixture_palette(primary: Colour) -> IndexedItemPalette {
    IndexedItemPalette { primary, secondary: Colour { palette: PaletteIndex(0), alpha: Alpha(0) }, secondary_source: ShapeSecondarySource::White }
}

fn view<'a>(rows: &'a [RowView<'a>]) -> MenuView<'a, 'a> {
    let background = Colour { palette: PaletteIndex(0), alpha: Alpha::OPAQUE };
    let selected = Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE };
    MenuView {
        rows, cursor: Some(RowIndex(0)), scroll_top: RowIndex(0),
        layout: MenuLayout { bounds: PanelRect::new(PanelPoint::new(0, 0).unwrap(), 40, 48).unwrap(), row_height: core::num::NonZeroU16::new(24).unwrap(), label_inset: 1, value_inset: 20 },
        style: MenuStyle { font: FontId(13), normal: background, selected, disabled: background, selection_background: selected, background },
    }
}

#[test]
fn menu_composition_rasterizes_and_queues_owned_pixels() {
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(99);
    let mut context = panel_actor(&BUFFERS, 1);
    let rows = [RowView { label: ResourceContent::Image { image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }, value: None, state: RowState::Enabled }];
    let mut commands = [DrawCommand::ResetClip; 16];
    let frame = compose_menu_and_publish(view(&rows), &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[0..4], &[0, 0, 255, 255]);
        assert_eq!(&pixels[4..8], &[255, 0, 0, 255]);
        assert_eq!(&pixels[(720 * 479 + 719) * 4..], &[0, 0, 0, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn missing_image_cancels_frame_and_allows_corrected_retry() {
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 2);
    let rows = [RowView { label: ResourceContent::Image { image: ImageId::new(8).unwrap(), alpha: Alpha::OPAQUE }, value: None, state: RowState::Enabled }];
    let mut commands = [DrawCommand::ResetClip; 16];
    assert_eq!(compose_menu_and_publish(view(&rows), &mut commands, &mut RasterResources::coverage8(&pack), &mut context), Err(FrameError::Actor(ActorError::Render(RenderError::Raster(RasterError::MissingImage(ImageId::new(8).unwrap()))))));
    let corrected = [RowView { label: ResourceContent::Image { image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }, value: None, state: RowState::Enabled }];
    assert!(compose_menu_and_publish(view(&corrected), &mut commands, &mut RasterResources::coverage8(&pack), &mut context).is_ok());
    assert_eq!(compose_menu_and_publish(view(&corrected), &mut commands, &mut RasterResources::coverage8(&pack), &mut context), Err(FrameError::Actor(ActorError::Busy)));
}

#[test]
fn unresolved_menu_semantics_and_capacity_never_queue_partial_frames() {
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 3);
    let rows = [RowView { label: ResourceContent::Unavailable(SemanticPrerequisite::MenuHook), value: None, state: RowState::Enabled }];
    let mut commands = [DrawCommand::ResetClip; 16];
    assert_eq!(compose_menu_and_publish(view(&rows), &mut commands, &mut RasterResources::coverage8(&pack), &mut context), Err(FrameError::Producer(ProducerError::MissingSemantic(SemanticPrerequisite::MenuHook))));
    let corrected = [RowView { label: ResourceContent::Image { image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }, value: None, state: RowState::Enabled }];
    assert_eq!(compose_menu_and_publish(view(&corrected), &mut commands[..1], &mut RasterResources::coverage8(&pack), &mut context), Err(FrameError::Producer(ProducerError::Capacity)));
    assert!(compose_menu_and_publish(view(&corrected), &mut commands, &mut RasterResources::coverage8(&pack), &mut context).is_ok());
}

#[test]
fn scrolled_hidden_and_partial_rows_do_not_resolve_unshown_resources() {
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 4);
    let unavailable = ResourceContent::Unavailable(SemanticPrerequisite::MenuHook);
    let rows = [
        RowView { label: unavailable, value: None, state: RowState::Enabled },
        RowView { label: unavailable, value: None, state: RowState::Hidden },
        RowView { label: ResourceContent::Image { image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }, value: None, state: RowState::Enabled },
        RowView { label: unavailable, value: None, state: RowState::Enabled },
    ];
    let mut menu = view(&rows);
    menu.cursor = Some(RowIndex(2));
    menu.scroll_top = RowIndex(1);
    menu.layout.bounds = PanelRect::new(PanelPoint::new(0, 0).unwrap(), 40, 25).unwrap();
    let mut commands = [DrawCommand::ResetClip; 16];
    let frame = compose_menu_and_publish(menu, &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[4..8], &[255, 0, 0, 255]);
        assert_eq!(&pixels[720 * 24 * 4..720 * 24 * 4 + 4], &[0, 0, 0, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn raw_stock_leaf_boundary_decodes_images_and_refuses_missing_metrics() {
    use crate::render_commands::producer::{classify_stock_resource, stock_grid_origin};
    use crate::render_commands::ResourceId;
    assert_eq!(classify_stock_resource(ResourceId(0xff000007)), Err(ProducerError::MissingSemantic(SemanticPrerequisite::BitmapKind(ResourceId(0xff000007)))));
    assert_eq!(stock_grid_origin(2, 3, ResourceId(0xff000007)), PanelPoint::new(20, 48).ok_or(ProducerError::Geometry));
    assert_eq!(stock_grid_origin(-20, 3, ResourceId(7)), Err(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(ResourceId(7)))));
    assert_eq!(classify_stock_resource(ResourceId(7)), Ok(ResourceContent::Text(crate::render_commands::TextRef::Catalog(CatalogTextId::new(7).unwrap()))));
    assert_eq!(classify_stock_resource(ResourceId(0xfd000000)), Ok(ResourceContent::Text(crate::render_commands::TextRef::Custom(CustomTextId::new(0).unwrap()))));
}

#[test]
fn existing_tree_producer_uses_owned_leaf_backend_and_refuses_unresolved_hooks() {
    use super::owned_tree::{compose_tree_and_publish, HookSemantics, SpecialMode, TreeData, TreeRow};
    use super::tree_list::{Child, DrawArgs, DrawMode, NestedGrid, Snapshot, Summary};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 5);
    let row = TreeRow { child: Child { record: 0, id: 42, selection: 0, variant: 0, state: 0, reserved: 0 }, label: ResourceId(0xff000007), value: ResourceId(0), hook: HookSemantics::Unresolved, palette_override: false };
    let unresolved_rows = [row];
    let resolved_rows = [TreeRow { hook: HookSemantics::Absent, ..row }];
    let colour = Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE };
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let mut data = TreeData {
        node: NodeId(1), snapshots: [Some(Snapshot([1, 0, 0, 42, 0, 0, 0, 0, 1, 1, 0])), None, None],
        summary: Summary { rows: 1, reserved: 0, page_capacity: 1, retained: [0; 3] }, rows: &unresolved_rows, texts: &[],
        bitmaps: &bitmaps,
        selection: super::owned_tree::SelectionResolution::FromRows, nested: &[], hooks: &[],
        placements: &[],
        saved_font: FontId(13), selected: fixture_palette(colour), ordinary: fixture_palette(colour), disabled: fixture_palette(colour), initial_primary_enabled: false, special_mode: SpecialMode::Ordinary,
    };
    let args = DrawArgs { node: NodeId(1), first: 2, second: 1, first_step: 0, second_step: 1, nested: NestedGrid { first: 1, second: 1, first_step: 0, second_step: 1 }, mode: DrawMode(0) };
    let mut report = DrawReport([0xa5; 0x44]);
    let mut commands = [DrawCommand::ResetClip; 16];
    assert_eq!(compose_tree_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut context), Err(FrameError::Producer(ProducerError::MissingSemantic(SemanticPrerequisite::MenuHook))));
    assert_eq!(report, DrawReport([0xa5; 0x44]));
    data.rows = &resolved_rows;
    let frame = compose_tree_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    assert_eq!(u32::from_le_bytes(report.0[12..16].try_into().unwrap()), 42);
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[20 * 4..21 * 4], &[255, 0, 0, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn nested_tree_keeps_hidden_rows_out_of_pixels_and_reports_selected_data() {
    use super::owned_tree::{compose_tree_and_publish, HookSemantics, NestedData, NestedRow, SelectionResolution, SpecialMode, TreeData, TreeRow};
    use super::tree_list::{Child, DrawArgs, DrawMode, NestedChild, NestedGrid, Snapshot, Summary};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 30);
    let colour = Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE };
    let outer = [TreeRow { child: Child { record: 17, id: 42, selection: 3, variant: 0, state: 0, reserved: 0 }, label: ResourceId(0xff000007), value: ResourceId(0), hook: HookSemantics::Absent, palette_override: false }];
    let footer = [DrawCommand::Fill { rect: PanelRect::new(PanelPoint::new(80, 72).unwrap(), 1, 1).unwrap(), colour }];
    let rows = [
        NestedRow { child: NestedChild { selection: 8, state: 3, data: 88 }, label: ResourceId(0xff000008), hook: HookSemantics::Unresolved, detail: None, footer: None },
        NestedRow { child: NestedChild { selection: 9, state: 0, data: 99 }, label: ResourceId(0xff000007), hook: HookSemantics::Absent, detail: Some(&[]), footer: Some(&footer) },
    ];
    let nested = [NestedData { parent: super::owned_tree::ChildId(42), summary: Summary { rows: 2, reserved: 0, page_capacity: 1, retained: [0; 3] }, rows: &rows }];
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let data = TreeData {
        node: NodeId(1), snapshots: [Some(Snapshot([2, 0, 0, 42, 9, 9, 0, 0, 1, 1, 0])), None, None],
        summary: Summary { rows: 1, reserved: 0, page_capacity: 1, retained: [0; 3] }, rows: &outer, texts: &[],
        bitmaps: &bitmaps,
        selection: SelectionResolution::FromRows, nested: &nested, hooks: &[],
        placements: &[],
        saved_font: FontId(13), selected: fixture_palette(colour), ordinary: fixture_palette(colour), disabled: fixture_palette(colour), initial_primary_enabled: false, special_mode: SpecialMode::Ordinary,
    };
    let args = DrawArgs { node: NodeId(1), first: 2, second: 1, first_step: 0, second_step: 1, nested: NestedGrid { first: 3, second: 2, first_step: 0, second_step: 1 }, mode: DrawMode(0) };
    let mut report = DrawReport::zeroed();
    let mut commands = [DrawCommand::ResetClip; 16];
    let frame = compose_tree_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    assert_eq!(u32::from_le_bytes(report.0[0x40..0x44].try_into().unwrap()), 99);
    assert_eq!(report.0[0x3c], 0);
    assert_eq!(report.0[0x3d], 0);
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[(24 * 720 + 40) * 4..(24 * 720 + 40) * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[(72 * 720 + 80) * 4..(72 * 720 + 80) * 4 + 4], &[0, 0, 255, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn picker_data_adapter_draws_visible_selection_and_keeps_skipped_modes_inert() {
    use super::owned_picker::{compose_picker_and_publish, PickerData, PickerMetrics, PickerRow, PickerStyle};
    use super::owned_tree::HookSemantics;
    use super::value_picker::{Child, DrawArgs, DrawMode, LabelBound, Palette, Snapshot, Summary};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 31);
    let rows = [
        PickerRow { child: Child { id: 8, state: 3, auxiliary: 88 }, labels: [ResourceId(0xff000008); 4], hook: HookSemantics::Unresolved },
        PickerRow { child: Child { id: 9, state: 0, auxiliary: 99 }, labels: [ResourceId(0), ResourceId(0xff000007), ResourceId(0), ResourceId(0)], hook: HookSemantics::Absent },
    ];
    let metrics = [PickerMetrics { resource: ResourceId(0xff000007), columns: [1; 2], size: [1; 2], extent: [1; 2] }];
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let data = PickerData {
        node: NodeId(1), snapshots: [Some(Snapshot { kind: 2, layout: 1, parent: 42, selected: 9, remembered: 9, rows: 1, page: 1, ..Snapshot::default() }), None],
        parent: super::owned_tree::ChildId(42), summary: Summary { children: 2, capacity: 1, ..Summary::default() }, rows: &rows, metrics: &metrics,
        placements: &[], texts: &[], hooks: &[], effects: &[], side_state: [0; 5], saved_font: FontId(13),
        bitmaps: &bitmaps,
        grid_palettes: None,
        initial_style: PickerStyle { font: FontId(13), palette: Palette { index: 0, alpha: 255, component2: 0, component3: 0 }, angle: false, primary_enabled: false },
        palette_component: 0, bar_style: 0, bar_resources: [ResourceId(0); 3], menu_mode: 0, help_hidden: false,
    };
    let mut args = DrawArgs { node: NodeId(1), x: 2, y: 1, width: 1, column_step: 0, row_step: 1, style: 0, scrolling: true, hide_help: 0, mode: DrawMode(2), label_bound: LabelBound::Stock };
    let mut report = DrawReport([0xa5; 0x44]);
    let mut commands = [DrawCommand::ResetClip; 16];
    assert_eq!(compose_picker_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut context), Ok(None));
    assert_eq!(report, DrawReport([0xa5; 0x44]));
    args.mode = DrawMode(0);
    let frame = compose_picker_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().unwrap().frames()[0].unwrap().frame;
    assert_eq!(u32::from_le_bytes(report.0[0x34..0x38].try_into().unwrap()), 9);
    assert_eq!(u32::from_le_bytes(report.0[0x40..0x44].try_into().unwrap()), 99);
    assert_eq!(report.0[0x3c], 0);
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[80..84], &[255, 0, 0, 255]);
        assert_eq!(&pixels[0..4], &[0, 0, 0, 0]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn actual_descriptor_centres_vertically_and_uses_catalog_position_on_natural_axis() {
    use super::owned_tree::{descriptor_origin, ResourcePlacement};
    use super::tree_list::ResourceDescriptor;
    use crate::render_commands::ResourceId;
    let placement = [ResourcePlacement { resource: ResourceId(7), position: [540, 12], extent: [31, 17] }];
    let mut descriptor = ResourceDescriptor { resource: ResourceId(7), x: -20, y: 24, width: 0, height: 72, reserved: [0; 3], layout: 2, tail: 0 };
    assert_eq!(descriptor_origin(&descriptor, &placement), Ok(PanelPoint::new(540, 40).unwrap()));
    descriptor.reserved[2] = 3;
    descriptor.x = 20;
    descriptor.width = 100;
    assert_eq!(descriptor_origin(&descriptor, &placement), Ok(PanelPoint::new(69, 40).unwrap()));
    assert_eq!(descriptor_origin(&descriptor, &[]), Err(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(ResourceId(7)))));
}

#[test]
fn catalog_metric_boundary_preserves_signed_values_and_disabled_zero() {
    use crate::render_commands::producer::{catalog_dimensions, catalog_metric, CatalogMetrics, MetricMode};
    use crate::render_commands::ResourceId;
    let records = [CatalogMetrics { id: CatalogTextId::new(7).unwrap(), dimensions: [20, 24], metrics: Some(&[-3, 9]) }];
    assert_eq!(catalog_dimensions(ResourceId(7), &records), Ok([20, 24]));
    assert_eq!(catalog_dimensions(ResourceId(0), &records), Ok([0, 0]));
    assert_eq!(catalog_metric(ResourceId(7), 0, MetricMode::Enabled, &records), Ok(-3));
    assert_eq!(catalog_metric(ResourceId(7), 1, MetricMode::Enabled, &records), Ok(9));
    assert_eq!(catalog_metric(ResourceId(8), 0, MetricMode::Disabled, &records), Ok(0));
    assert_eq!(catalog_metric(ResourceId(0x83c), 0, MetricMode::Enabled, &records), Ok(0));
    assert_eq!(catalog_metric(ResourceId(8), 0, MetricMode::Enabled, &records), Err(ProducerError::MissingSemantic(SemanticPrerequisite::ResourceMetrics(ResourceId(8)))));
}

#[test]
fn borrowed_firmware_catalog_selects_language_variant_and_decodes_text() {
    use crate::render_commands::resources::{CatalogSet, DataAddress, DataRegion, FontLanguage, LanguageCatalog, LanguageId, ResourceMemory, TextVariant};
    let mut bytes = [0u8; 192];
    bytes[10] = 2;
    bytes[12..16].copy_from_slice(&0x1040u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&0x1080u32.to_le_bytes());
    bytes[76..80].copy_from_slice(&0x1090u32.to_le_bytes());
    bytes[128..132].copy_from_slice(&[b'A', 0, 0, 0]);
    bytes[144..148].copy_from_slice(&[b'B', 0, 0, 0]);
    let regions = [DataRegion { base: DataAddress(0x1000), bytes: &bytes }];
    let catalogs = [LanguageCatalog { language: LanguageId::new(1), records: DataAddress(0x1000) }];
    let set = CatalogSet { memory: ResourceMemory::new(&regions).unwrap(), catalogs: &catalogs };
    let mut output = [0u8; 16];
    assert_eq!(set.text(LanguageId::new(1), CatalogTextId::new(1).unwrap(), TextVariant(0), &mut output), Ok("A"));
    assert_eq!(set.text(LanguageId::new(99), CatalogTextId::new(1).unwrap(), TextVariant(1), &mut output), Ok("B"));
    assert_eq!(LanguageId::new(0).font_class(), FontLanguage::Three);
    assert_eq!(LanguageId::new(99).value(), 1);
}

#[test]
fn inline_format_updates_palette_font_and_emits_encoded_text_without_callbacks() {
    use crate::render_commands::resources::{emission_text, EmittedText, FormatAction, FormatArgument, FormatEmission, FormatState, PaletteOptions, ResourceError};
    let colour = Colour { palette: PaletteIndex(0), alpha: Alpha::OPAQUE };
    let initial = FormatState::new(FontId(0), colour, colour);
    let mut emissions = [FormatEmission { state: initial, action: FormatAction::Text(EmittedText::Bytes(b"")) }; 4];
    let (state, count) = initial.parse_stock(b"%2c%ip\0", FormatArgument::Scalar { primary: 13, secondary: 0 }, &mut emissions).unwrap();
    assert_eq!(state.primary, PaletteIndex(2));
    assert_eq!(state.font, FontId(13));
    assert_eq!(count, 0);
    let (_, count) = state.parse_stock(b"%s\0", FormatArgument::Bytes(b"A\0"), &mut emissions).unwrap();
    let mut output = [0u8; 8];
    assert_eq!(count, 1);
    assert_eq!(emission_text(format_text(emissions[0]), &mut output), Ok("\u{e141}"));
    assert!(matches!(initial.parse_stock(b"%700x\0", FormatArgument::Scalar { primary: 0, secondary: 0 }, &mut emissions), Err(ResourceError::InvalidFormat)));
    let (_, count) = initial.parse_stock(b"ABC\0", FormatArgument::Scalar { primary: 0, secondary: 0 }, &mut emissions).unwrap();
    let mut literal_output = [0u8; 16];
    assert_eq!(count, 3);
    assert_eq!(emission_text(format_text(emissions[0]), &mut literal_output), Ok("\u{e141}\u{e142}\u{e143}"));
    assert_eq!(emission_text(format_text(emissions[1]), &mut literal_output), Ok("\u{e142}\u{e143}"));
    assert_eq!(emission_text(format_text(emissions[2]), &mut literal_output), Ok("\u{e143}"));
    let mut palette = [0u8; 0x30];
    palette[0x11] = 0x60;
    palette[0x1a..0x1c].copy_from_slice(&9u16.to_le_bytes());
    palette[0x24..0x26].copy_from_slice(&720u16.to_le_bytes());
    let options = PaletteOptions::parse(&palette).unwrap();
    assert!(options.enabled && options.style);
    assert_eq!(options.palette, PaletteIndex(9));
    assert_eq!(options.bounds, [720, 0]);
}

#[test]
fn parsed_text_continues_by_glyph_advance_and_explicit_same_position_resets() {
    use crate::render_commands::producer::CommandBuffer;
    use crate::render_commands::resources::{EmittedText, FormatAction, FormatArgument, FormatEmission, FormatState};
    use crate::render_commands::context::OutputMode;
    use crate::render_commands::runtime::render_output;
    let mut bytes = b"RCP1".to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    record(&mut bytes, 1, 0, [0; 3], &[255, 255, 255, 255]);
    let glyph = [
        0,255,255,255,0,
        255,0,0,0,255,
        255,0,0,0,255,
        255,255,255,255,255,
        255,0,0,0,255,
        255,0,0,0,255,
        255,0,0,0,255,
    ];
    let start = bytes.len();
    record(&mut bytes, 3, 13, [5, 7, 5], &glyph);
    bytes[start + 8..start + 12].copy_from_slice(&0xe141u32.to_le_bytes());
    bytes[start + 18..start + 20].copy_from_slice(&6i16.to_le_bytes());
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image original five-by-seven capital A" }).unwrap();
    static ADJACENT: StaticBuffers = StaticBuffers::new(0);
    static RESET: StaticBuffers = StaticBuffers::new(0);
    static OPAQUE: StaticBuffers = StaticBuffers::new(0);
    static SET_Y: StaticBuffers = StaticBuffers::new(0);
    static ADVANCE: StaticBuffers = StaticBuffers::new(0);
    static SPLIT: StaticBuffers = StaticBuffers::new(0);
    for (buffers, identity, format, first_alpha, second_offset, split) in [
        (&ADJACENT, 6, b"%128t%s%s\0".as_slice(), 128, Some(28), false),
        (&RESET, 7, b"%128t%s%0x%s\0".as_slice(), 192, None, false),
        (&OPAQUE, 9, b"%s%s\0".as_slice(), 255, Some(28), false),
        (&SET_Y, 10, b"%128t%s%10y%s\0".as_slice(), 128, Some((720 * 10 + 7) * 4), false),
        (&ADVANCE, 11, b"%128t%s%1m%s\0".as_slice(), 128, Some(108), false),
        (&SPLIT, 12, b"%128t%s\0".as_slice(), 128, Some(28), true),
    ] {
        let mut context = panel_context(buffers, identity);
        let colour = Colour { palette: PaletteIndex(0), alpha: Alpha::OPAQUE };
        let state = FormatState::new(FontId(13), colour, colour);
        let mut emissions = [FormatEmission { state, action: FormatAction::Text(EmittedText::Bytes(b"")) }; 4];
        let (state, mut count) = state.parse_stock(format, FormatArgument::Bytes(b"A\0"), &mut emissions).unwrap();
        if split {
            count += state.parse_stock(b"%s\0", FormatArgument::Bytes(b"A\0"), &mut emissions[count..]).unwrap().1;
        }
        let mut commands = [DrawCommand::ResetClip; 16];
        let mut strings = [[0u8; 128]; 4];
        let mut buffer = CommandBuffer::new(&mut commands);
        buffer.push(DrawCommand::TextPen(crate::render_commands::commands::TextPenOp::Set(PanelPoint::new(0, 0).unwrap()))).unwrap();
        buffer.stock_formatted(&emissions[..count], &mut strings, Alpha::OPAQUE).unwrap();
        let frame = render_output(&mut context, &mut RasterResources::coverage8(&pack), OutputMode::Panel, buffer.draw_list(), None).unwrap().frames()[0].unwrap().frame;
        context.surface(SurfaceId::Osd).unwrap().submit(frame, |_, _, pixels| {
            assert_eq!(&pixels[4..8], &[255, 255, 255, first_alpha]);
            if let Some(offset) = second_offset {
                assert_eq!(&pixels[offset..offset + 4], &[255, 255, 255, if first_alpha == 255 { 255 } else { 128 }]);
            } else {
                assert_eq!(&pixels[28..32], &[0, 0, 0, 0]);
            }
            Ok::<(), SubmissionError<()>>(())
        }).unwrap().unwrap();
    }
    static COLUMN_VALID: StaticBuffers = StaticBuffers::new(0);
    static COLUMN_INVALID: StaticBuffers = StaticBuffers::new(0);
    for (buffers, identity, format, valid) in [
        (&COLUMN_VALID, 13, b"%19x%32m%s\0".as_slice(), true),
        (&COLUMN_INVALID, 14, b"%20x%32m%s\0".as_slice(), false),
    ] {
        let mut context = panel_context(buffers, identity);
        let colour = Colour { palette: PaletteIndex(0), alpha: Alpha::OPAQUE };
        let state = FormatState::new(FontId(13), colour, colour);
        let mut emissions = [FormatEmission { state, action: FormatAction::Text(EmittedText::Bytes(b"")) }; 4];
        let count = state.parse_stock(format, FormatArgument::Bytes(b"A\0"), &mut emissions).unwrap().1;
        let mut commands = [DrawCommand::ResetClip; 16];
        let mut strings = [[0u8; 128]; 4];
        let mut buffer = CommandBuffer::new(&mut commands);
        buffer.push(DrawCommand::TextPen(crate::render_commands::commands::TextPenOp::Set(PanelPoint::new(0, 0).unwrap()))).unwrap();
        buffer.stock_formatted(&emissions[..count], &mut strings, Alpha::OPAQUE).unwrap();
        let result = render_output(&mut context, &mut RasterResources::coverage8(&pack), OutputMode::Panel, buffer.draw_list(), None);
        if valid {
            let frame = result.unwrap().frames()[0].unwrap().frame;
            context.surface(SurfaceId::Osd).unwrap().submit(frame, |_, _, pixels| {
                assert_eq!(&pixels[660 * 4..661 * 4], &[255, 255, 255, 255]);
                Ok::<(), SubmissionError<()>>(())
            }).unwrap().unwrap();
        } else {
            assert_eq!(result, Err(RenderError::Raster(RasterError::TextPenOutOfBounds)));
            assert_eq!(context.surface(SurfaceId::Osd).unwrap().queued(), None);
        }
    }
}

#[test]
fn canonical_menu_engine_state_supplies_rows_and_visible_scroll_to_cpu_output() {
    use super::engine_frame::{compose_engine_menu_and_publish, LabelRole, RowValues};
    use super::owned_tree::HookSemantics;
    use crate::menu_engine::frame::{MenuClass, NodeId, NodeType, RowIndex as EngineRowIndex, RowToken, RowsPerPage};
    use crate::menu_engine::state::{EngineState, MenuOpen, RowState as EngineRowState};
    use crate::menu_engine::tree::{Availability, Label, Node, NodeRef, NodeWord10, Remember, Row, RowChild, StringId, Tree, TreeIdentity, Wrap, REGISTRY_SLOTS};
    static ROWS: [Row; 2] = [
        Row { token: RowToken::new(10), child: RowChild::Leaf },
        Row { token: RowToken::new(20), child: RowChild::Leaf },
    ];
    static NODES: [Node; 1] = [Node {
        id: NodeId::new(1), node_type: NodeType::new(1), class: MenuClass::MENU_PAGE,
        rows_per_page: RowsPerPage::new(1), wrap: Wrap::Stops, availability: Availability::Fixed,
        remember: Remember::Off, parent: None, word10: NodeWord10::new(0), rows: Some(&ROWS),
    }];
    let tree = Tree { nodes: &NODES, registry: [None; REGISTRY_SLOTS], identity: TreeIdentity([0; 32]) };
    let mut state = EngineState::EMPTY;
    state.node = Some(NodeRef::new(0));
    state.menu_open = MenuOpen::Open;
    state.row_states[0] = EngineRowState::Hidden;
    state.live.cursor = EngineRowIndex::new(1);
    let label = Label { name: StringId::new(7), list_icon: StringId::new(0xff000007), preview_icon: StringId::new(0), help: StringId::new(0) };
    let label_rows = [label; 2];
    let labels = [Some(label_rows.as_slice())];
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 8);
    let empty = RowView { label: ResourceContent::Unavailable(SemanticPrerequisite::MenuData), value: None, state: RowState::Hidden };
    let mut rows = [empty; 2];
    let menu = view(&[]);
    let mut commands = [DrawCommand::ResetClip; 16];
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let frame = compose_engine_menu_and_publish(&tree, &state, &labels, &bitmaps, &[None; 2], LabelRole::ListIcon, &[HookSemantics::Absent; 2], RowValues::NotDisplayed, &mut rows, menu.layout, menu.style, &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[4..8], &[255, 0, 0, 255]);
        assert_eq!(&pixels[720 * 24 * 4..720 * 24 * 4 + 4], &[0, 0, 0, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn native_bitmap_descriptor_extracts_data_only_pixels_into_cpu_frame() {
    use crate::render_commands::commands::DrawList;
    use crate::render_commands::context::OutputMode;
    use crate::render_commands::resources::{BitmapCatalog, DataAddress, DataRegion, ResourceMemory};
    use crate::render_commands::runtime::render_output;
    use crate::render_commands::ResourceId;
    let mut record = [0u8; 12];
    record[..2].copy_from_slice(&2u16.to_le_bytes());
    record[2..4].copy_from_slice(&1u16.to_le_bytes());
    record[8..12].copy_from_slice(&0x00300000u32.to_le_bytes());
    let pixels = [0xff, 0x33, 0x22, 0x11, 0x80, 0xef, 0xcd, 0xab];
    let regions = [
        DataRegion { base: DataAddress(0x0022a908), bytes: &record },
        DataRegion { base: DataAddress(0x00300000), bytes: &pixels },
    ];
    let catalog = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let mut output = [0u8; 40];
    let pack = catalog.extract_rcp1(ResourceId(0xff000007), AssetProvenance::Original { author: "Latent Image synthetic native RGBA descriptor" }, &mut output).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_context(&BUFFERS, 15);
    let commands = [DrawCommand::Image { origin: PanelPoint::new(0, 0).unwrap(), image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }];
    let frame = render_output(&mut context, &mut RasterResources::coverage8(&pack), OutputMode::Panel, DrawList::new(SurfaceId::Osd, &commands), None).unwrap().frames()[0].unwrap().frame;
    context.surface(SurfaceId::Osd).unwrap().submit(frame, |_, _, pixels| {
        assert_eq!(&pixels[..8], &[0x11, 0x22, 0x33, 0xff, 0xab, 0xcd, 0xef, 0x80]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn pane_composition_draws_root_and_chrome_without_stock_services() {
    use super::owned_pane::{compose_pane_and_publish, PaneData, PanePalette, PaneRoot, PaneStyle};
    use super::owned_tree::HookSemantics;
    use super::pane_drawer::{DrawMode, PaneState, RootChild};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    let mut bytes = b"RCP1".to_vec();
    bytes.extend_from_slice(&5u32.to_le_bytes());
    record(&mut bytes, 1, 0, [0; 3], &[0, 0, 0, 255]);
    record(&mut bytes, 1, 1, [0; 3], &[255, 255, 255, 255]);
    record(&mut bytes, 2, 7, [1, 1, 4], &[255, 0, 0, 255]);
    record(&mut bytes, 2, 0xe7, [1, 1, 4], &[0, 255, 0, 255]);
    record(&mut bytes, 2, 0xf3, [1, 1, 4], &[0, 0, 255, 255]);
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic pane colour swatches" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = panel_actor(&BUFFERS, 32);
    let colour = Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE };
    let roots = [PaneRoot {
        child: RootChild { record: 17, child: 42, selection: 3, variant: 0, state: 0, suppress_value: 0 },
        labels: [ResourceId(0xff000007); 2], value: ResourceId(0), special: false,
        label_hook: HookSemantics::Absent, value_hook: HookSemantics::Unresolved,
    }];
    let palettes = [PanePalette { nested: false, selected: true, state: 0, palette: fixture_palette(colour), primary_enabled: false }];
    let mut report = DrawReport::zeroed();
    report.put_word(0, 1).unwrap();
    report.put_word(4, 1).unwrap();
    report.put_word(12, 42).unwrap();
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let data = PaneData {
        node: NodeId(1), mode: DrawMode(0), root: NodeId(1),
        state: PaneState { side: [3, 1, 1, 2, 1, 0, 0], summary: [1, 1, 1], report },
        roots: &roots, nested: &[], palettes: &palettes, hooks: &[], effects: &[], tabs: &[], texts: &[],
        bitmaps: &bitmaps,
        grid_palettes: None,
        saved_font: FontId(13), initial_style: PaneStyle { font: FontId(13), palette: fixture_palette(colour), primary_enabled: false },
        pixel_display: false, wide_display: false, title_suppressed: true, title: ResourceId(0),
        tab_getters: [0; 4], markers: [ResourceId(0); 2], remembered_colour: 0,
    };
    let mut commands = [DrawCommand::ResetClip; 16];
    let frame = compose_pane_and_publish(&data, NodeId(1), DrawMode(0), &mut commands, &mut RasterResources::coverage8(&pack), &mut context).unwrap().frames()[0].unwrap().frame;
    context.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[(72 * 720 + 100) * 4..(72 * 720 + 100) * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[(48 * 720 + 60) * 4..(48 * 720 + 60) * 4 + 4], &[0, 255, 0, 255]);
        assert_eq!(&pixels[(72 * 720 + 80) * 4..(72 * 720 + 80) * 4 + 4], &[0, 0, 255, 255]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn resolved_tree_hooks_replace_unavailable_resource_with_owned_pixels() {
    use super::owned_tree::{compose_tree_and_publish, HookDisposition, HookSemantics, HookStage, ResolvedHook, SelectionResolution, SpecialMode, TreeData, TreeRow};
    use super::tree_list::{Child, DrawArgs, DrawMode, HookRecord, NestedGrid, Snapshot, Summary};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    let bytes = assets();
    let pack = AssetPack::parse(&bytes, AssetProvenance::Original { author: "Latent Image synthetic colour-swatch scenario" }).unwrap();
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut actor = panel_actor(&BUFFERS, 33);
    let colour = Colour { palette: PaletteIndex(1), alpha: Alpha::OPAQUE };
    let rows = [TreeRow { child: Child { record: 17, id: 42, selection: 3, variant: 0, state: 0, reserved: 0 }, label: ResourceId(0xff000008), value: ResourceId(0), hook: HookSemantics::Unresolved, palette_override: false }];
    let image = [DrawCommand::Image { origin: PanelPoint::new(60, 0).unwrap(), image: ImageId::new(7).unwrap(), alpha: Alpha::OPAQUE }];
    let label_record = HookRecord([1, 42, 0, 0, 0, 0, 0, 0, 2, 1, 0, 1, 0xff000008, 1, 0, 0]);
    let value_record = HookRecord([1, 42, 0, 0, 0, 0, 0, 0, 2, 1, 0, 1, 0, 1, 0, 0]);
    let hooks = [
        ResolvedHook { stage: HookStage::Label, input: label_record, output: label_record, input_font: FontId(13), input_palette: fixture_palette(colour), input_primary_enabled: false, output_font: FontId(13), output_palette: fixture_palette(colour), output_primary_enabled: false, disposition: HookDisposition::Handled, commands: &image },
        ResolvedHook { stage: HookStage::Value, input: value_record, output: value_record, input_font: FontId(13), input_palette: fixture_palette(colour), input_primary_enabled: false, output_font: FontId(13), output_palette: fixture_palette(colour), output_primary_enabled: false, disposition: HookDisposition::Default, commands: &[] },
    ];
    let bitmap_bytes = bitmap_table();
    let regions = [DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes }];
    let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
    let data = TreeData {
        node: NodeId(1), snapshots: [Some(Snapshot([1, 0, 0, 42, 0, 0, 0, 0, 1, 1, 0])), None, None],
        summary: Summary { rows: 1, reserved: 0, page_capacity: 1, retained: [0; 3] }, rows: &rows, texts: &[],
        bitmaps: &bitmaps,
        selection: SelectionResolution::FromRows, nested: &[], hooks: &hooks, placements: &[],
        saved_font: FontId(13), selected: fixture_palette(colour), ordinary: fixture_palette(colour), disabled: fixture_palette(colour), initial_primary_enabled: false, special_mode: SpecialMode::Ordinary,
    };
    let args = DrawArgs { node: NodeId(1), first: 2, second: 1, first_step: 0, second_step: 1, nested: NestedGrid { first: 1, second: 1, first_step: 0, second_step: 1 }, mode: DrawMode(0) };
    let mut report = DrawReport::zeroed();
    let mut commands = [DrawCommand::ResetClip; 16];
    let frame = compose_tree_and_publish(&data, args, &mut report, &mut commands, &mut RasterResources::coverage8(&pack), &mut actor).unwrap().frames()[0].unwrap().frame;
    actor.submit(SurfaceId::Osd, frame, |_, _, pixels| {
        assert_eq!(&pixels[60 * 4..61 * 4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[20 * 4..21 * 4], &[0, 0, 0, 0]);
        Ok::<(), SubmissionError<()>>(())
    }).unwrap().unwrap();
}

#[test]
fn grid_palette_selector_loads_all_four_components_with_declared_provenance() {
    use super::owned_picker::{GridPaletteError, GridPaletteId, GridPaletteTable};
    use super::value_picker::Palette;
    let mut bytes = [0u8; 121 * 16];
    bytes[112..128].copy_from_slice(&[3, 0, 0, 0, 200, 0, 0, 0, 7, 0, 0, 0, 99, 0, 0, 0]);
    let provenance = AssetProvenance::Original { author: "Latent Image synthetic four-component palette table" };
    let table = GridPaletteTable::parse(&bytes, provenance).unwrap();
    assert_eq!(table.palette(GridPaletteId::new(7).unwrap()), Palette { index: 3, alpha: 200, component2: 7, component3: 99 });
    assert_eq!(table.provenance(), provenance);
    assert!(GridPaletteId::new(120).is_some());
    assert_eq!(GridPaletteId::new(121), None);
    assert!(matches!(GridPaletteTable::parse(&bytes[..1935], provenance), Err(GridPaletteError::Length)));
    assert!(matches!(GridPaletteTable::parse(&bytes, AssetProvenance::PrivateExtract { source: "" }), Err(GridPaletteError::Provenance)));
}

#[test]
fn tree_and_picker_native_labels_resolve_both_shape_kinds_to_gvff_pixels() {
    use super::owned_picker::{compose_picker_and_publish, PickerData, PickerMetrics, PickerRow, PickerStyle};
    use super::owned_tree::{compose_tree_and_publish, ChildId, HookSemantics, SelectionResolution, SpecialMode, TreeData, TreeRow};
    use super::{tree_list as tree, value_picker as picker};
    use crate::render_commands::{DrawReport, NodeId, ResourceId};
    use crate::render_commands::raster::{FontRegistry, VectorWorkspace};
    use crate::render_commands::resources::TextBinding;
    use crate::render_commands::vector_raster::{CurveAccuracy, DashBudget, SampleGrid, Vertex};
    enum Caller { Tree, Picker }
    enum SourcePaint { Black, White, Red, Green }
    static TREE_ONE: StaticBuffers = StaticBuffers::new(0);
    static TREE_TWO: StaticBuffers = StaticBuffers::new(0);
    static PICKER_ONE: StaticBuffers = StaticBuffers::new(0);
    static PICKER_TWO: StaticBuffers = StaticBuffers::new(0);
    static TREE_WHITE_ONE: StaticBuffers = StaticBuffers::new(0);
    static TREE_WHITE_TWO: StaticBuffers = StaticBuffers::new(0);
    static PICKER_WHITE_ONE: StaticBuffers = StaticBuffers::new(0);
    static PICKER_WHITE_TWO: StaticBuffers = StaticBuffers::new(0);
    static TREE_RED_ONE: StaticBuffers = StaticBuffers::new(0);
    static TREE_RED_TWO: StaticBuffers = StaticBuffers::new(0);
    static TREE_WHITE_RED_MODE: StaticBuffers = StaticBuffers::new(0);
    static TREE_RED_WHITE_MODE: StaticBuffers = StaticBuffers::new(0);
    static TREE_GREEN_ONE: StaticBuffers = StaticBuffers::new(0);
    static PICKER_GREEN_ONE: StaticBuffers = StaticBuffers::new(0);
    for (buffers, kind, caller, paint, secondary_source, identity, expected) in [
        (&TREE_ONE, 1u32, Caller::Tree, SourcePaint::Black, ShapeSecondarySource::White, 34, [0, 255, 0, 128]),
        (&TREE_TWO, 2, Caller::Tree, SourcePaint::Black, ShapeSecondarySource::White, 35, [0, 0, 0, 255]),
        (&PICKER_ONE, 1, Caller::Picker, SourcePaint::Black, ShapeSecondarySource::White, 36, [0, 255, 0, 255]),
        (&PICKER_TWO, 2, Caller::Picker, SourcePaint::Black, ShapeSecondarySource::White, 37, [0, 0, 0, 255]),
        (&TREE_WHITE_ONE, 1, Caller::Tree, SourcePaint::White, ShapeSecondarySource::White, 38, [0, 0, 255, 64]),
        (&TREE_WHITE_TWO, 2, Caller::Tree, SourcePaint::White, ShapeSecondarySource::White, 39, [255, 255, 255, 255]),
        (&PICKER_WHITE_ONE, 1, Caller::Picker, SourcePaint::White, ShapeSecondarySource::White, 40, [0, 0, 0, 0]),
        (&PICKER_WHITE_TWO, 2, Caller::Picker, SourcePaint::White, ShapeSecondarySource::White, 41, [255, 255, 255, 255]),
        (&TREE_RED_ONE, 1, Caller::Tree, SourcePaint::Red, ShapeSecondarySource::Red, 42, [0, 0, 255, 64]),
        (&TREE_RED_TWO, 2, Caller::Tree, SourcePaint::Red, ShapeSecondarySource::Red, 43, [255, 0, 0, 255]),
        (&TREE_WHITE_RED_MODE, 1, Caller::Tree, SourcePaint::White, ShapeSecondarySource::Red, 44, [0, 0, 0, 0]),
        (&TREE_RED_WHITE_MODE, 1, Caller::Tree, SourcePaint::Red, ShapeSecondarySource::White, 45, [0, 0, 0, 0]),
        (&TREE_GREEN_ONE, 1, Caller::Tree, SourcePaint::Green, ShapeSecondarySource::White, 46, [0, 255, 0, 255]),
        (&PICKER_GREEN_ONE, 1, Caller::Picker, SourcePaint::Green, ShapeSecondarySource::White, 47, [0, 255, 0, 255]),
    ] {
        let mut shape_bytes = std::vec![0u8; 184];
        shape_bytes[..4].copy_from_slice(b"GVFF");
        shape_bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        shape_bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
        shape_bytes[48..52].copy_from_slice(&4.0f32.to_le_bytes());
        shape_bytes[52..56].copy_from_slice(&(-3.0f32).to_le_bytes());
        shape_bytes[60..64].copy_from_slice(&0x1b00u32.to_le_bytes());
        if matches!(paint, SourcePaint::White) {
            for offset in [64, 68, 72] { shape_bytes[offset..offset + 4].copy_from_slice(&1.0f32.to_le_bytes()); }
        }
        if matches!(paint, SourcePaint::Red) { shape_bytes[64..68].copy_from_slice(&1.0f32.to_le_bytes()); }
        if matches!(paint, SourcePaint::Green) { shape_bytes[68..72].copy_from_slice(&1.0f32.to_le_bytes()); }
        shape_bytes[76..80].copy_from_slice(&1.0f32.to_le_bytes());
        shape_bytes[92..96].copy_from_slice(&2u32.to_le_bytes());
        shape_bytes[112..116].copy_from_slice(&0x29au32.to_le_bytes());
        shape_bytes[116..120].copy_from_slice(&0x1900u32.to_le_bytes());
        shape_bytes[120..124].copy_from_slice(&0x1800u32.to_le_bytes());
        shape_bytes[124..128].copy_from_slice(&0x1700u32.to_le_bytes());
        shape_bytes[136..140].copy_from_slice(&5u32.to_le_bytes());
        shape_bytes[140..144].copy_from_slice(&32u32.to_le_bytes());
        shape_bytes[144..152].copy_from_slice(&[2, 4, 4, 4, 0, 0, 0, 0]);
        for (index, value) in [0.0f32, 0.0, 4.0, 0.0, 4.0, -3.0, 0.0, -3.0].into_iter().enumerate() {
            shape_bytes[152 + index * 4..156 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut bitmap_bytes = bitmap_table();
        bitmap_bytes[84..88].copy_from_slice(&[4, 0, 3, 0]);
        bitmap_bytes[88..92].copy_from_slice(&kind.to_le_bytes());
        bitmap_bytes[92..96].copy_from_slice(&0x1000u32.to_le_bytes());
        let mut rgb = [0u8; 106 * 3];
        rgb[..6].copy_from_slice(&[0, 255, 0, 0, 0, 255]);
        rgb[315..318].copy_from_slice(&[0, 255, 0]);
        let regions = [
            DataRegion { base: DataAddress(0x1000), bytes: &shape_bytes },
            DataRegion { base: DataAddress(0x22a8b4), bytes: &bitmap_bytes },
            DataRegion { base: DataAddress(0x7a1f24), bytes: &rgb },
        ];
        let bitmaps = BitmapCatalog { memory: ResourceMemory::new(&regions).unwrap() };
        let shapes = [bitmaps.shape_binding(ResourceId(0xff000007)).unwrap()];
        let pack_bytes = *b"RCP1\0\0\0\0";
        let pack = AssetPack::parse(&pack_bytes, AssetProvenance::Original { author: "Latent Image original four-by-three solid rectangle scenarios" }).unwrap();
        let mut vertices = [Vertex::EMPTY; 5];
        let mut strokes = [];
        let workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, dash_budget: DashBudget::new(64), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::One };
        let mut resources = RasterResources::new(&pack, FontRegistry::Coverage8, &shapes, Some(workspace)).unwrap();
        let mut actor = panel_actor(buffers, identity);
        let mut commands = [DrawCommand::ResetClip; 16];
        let mut report = DrawReport::zeroed();
        let palette = IndexedItemPalette {
            primary: Colour { palette: PaletteIndex(0), alpha: Alpha(128) },
            secondary: Colour { palette: PaletteIndex(1), alpha: Alpha(64) },
            secondary_source,
        };
        let texts = [TextBinding { resource: ResourceId(0xff000007), text: "not a text resource" }];
        let batch = match caller {
            Caller::Tree => {
                let rows = [TreeRow { child: tree::Child { record: 0, id: 42, selection: 0, variant: 0, state: 0, reserved: 0 }, label: ResourceId(0xff000007), value: ResourceId(0), hook: HookSemantics::Absent, palette_override: false }];
                let data = TreeData {
                    node: NodeId(1), snapshots: [Some(tree::Snapshot([1, 0, 0, 42, 0, 0, 0, 0, 1, 1, 0])), None, None],
                    summary: tree::Summary { rows: 1, reserved: 0, page_capacity: 1, retained: [0; 3] }, rows: &rows, texts: &texts, bitmaps: &bitmaps,
                    selection: SelectionResolution::FromRows, nested: &[], hooks: &[], placements: &[],
                    saved_font: FontId(13), selected: palette, ordinary: palette, disabled: palette, initial_primary_enabled: true, special_mode: SpecialMode::Ordinary,
                };
                let args = tree::DrawArgs { node: NodeId(1), first: 2, second: 1, first_step: 0, second_step: 1, nested: tree::NestedGrid { first: 1, second: 1, first_step: 0, second_step: 1 }, mode: tree::DrawMode(0) };
                compose_tree_and_publish(&data, args, &mut report, &mut commands, &mut resources, &mut actor).unwrap()
            }
            Caller::Picker => {
                let rows = [PickerRow { child: picker::Child { id: 9, state: 0, auxiliary: 0 }, labels: [ResourceId(0), ResourceId(0xff000007), ResourceId(0), ResourceId(0)], hook: HookSemantics::Absent }];
                let metrics = [PickerMetrics { resource: ResourceId(0xff000007), columns: [1; 2], size: [1; 2], extent: [1; 2] }];
                let data = PickerData {
                    node: NodeId(1), snapshots: [Some(picker::Snapshot { kind: 2, layout: 1, parent: 42, selected: 9, rows: 1, page: 1, ..picker::Snapshot::default() }), None],
                    parent: ChildId(42), summary: picker::Summary { children: 1, capacity: 1, ..picker::Summary::default() }, rows: &rows, metrics: &metrics,
                    placements: &[], texts: &texts, bitmaps: &bitmaps, hooks: &[], effects: &[], grid_palettes: None, side_state: [0; 5], saved_font: FontId(13),
                    initial_style: PickerStyle { font: FontId(13), palette: picker::Palette { index: 0, alpha: 255, component2: 0, component3: 0 }, angle: false, primary_enabled: true },
                    palette_component: 0, bar_style: 0, bar_resources: [ResourceId(0); 3], menu_mode: 0, help_hidden: false,
                };
                let args = picker::DrawArgs { node: NodeId(1), x: 2, y: 1, width: 1, column_step: 0, row_step: 1, style: 0, scrolling: true, hide_help: 0, mode: picker::DrawMode(0), label_bound: picker::LabelBound::Stock };
                compose_picker_and_publish(&data, args, &mut report, &mut commands, &mut resources, &mut actor).unwrap().unwrap()
            }
        };
        actor.submit(SurfaceId::Osd, batch.frames()[0].unwrap().frame, |_, _, pixels| {
            assert_eq!(&pixels[20 * 4..21 * 4], &expected);
            assert_eq!(&pixels[(2 * 720 + 23) * 4..(2 * 720 + 23) * 4 + 4], &expected);
            assert_eq!(&pixels[24 * 4..25 * 4], &[0, 0, 0, 0]);
            assert_eq!(&pixels[(3 * 720 + 20) * 4..(3 * 720 + 20) * 4 + 4], &[0, 0, 0, 0]);
            Ok::<(), SubmissionError<()>>(())
        }).unwrap().unwrap();
    }
}
