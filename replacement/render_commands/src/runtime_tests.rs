use super::commands::{Alpha, Colour, DrawCommand, DrawList};
use super::context::{GraphicsContext, OutputMode};
use super::publisher::{ContextId, DrainState, Publisher, SubmissionError};
use super::raster::{AssetPack, AssetProvenance, RasterError, RasterResources};
use super::runtime::{render_output, RenderError};
use super::test_buffers::StaticBuffers;
use super::{PaletteIndex, PanelPoint, PanelRect, SurfaceId};

const PACK: &[u8] = &[
    b'R', b'C', b'P', b'1', 1, 0, 0, 0,
    1, 0, 0, 0, 1, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 4, 0, 0, 0,
    255, 0, 0, 255,
];

fn fill(surface: SurfaceId, x: u16, y: u16, palette: u32) -> DrawCommand<'static> {
    DrawCommand::Fill {
        rect: PanelRect::new_for(surface, PanelPoint::new_for(surface, x, y).unwrap(), 1, 1).unwrap(),
        colour: Colour { palette: PaletteIndex(palette), alpha: Alpha::OPAQUE },
    }
}

#[test]
fn complete_dual_output_renders_distinct_content_and_queues_without_presenting() {
    static PANEL: StaticBuffers = StaticBuffers::new(0);
    static WIDE: StaticBuffers<{ 960 * 544 * 4 }> = StaticBuffers::new(0);
    let [a, b] = PANEL.take();
    let [c, d] = WIDE.take();
    let mut context = GraphicsContext::new();
    assert!(context.install(Publisher::new(ContextId::new(20).unwrap(), SurfaceId::Osd, a, b).unwrap()).is_ok());
    assert!(context.install(Publisher::new(ContextId::new(21).unwrap(), SurfaceId::Stage3, c, d).unwrap()).is_ok());
    let assets = AssetPack::parse(PACK, AssetProvenance::Original { author: "Latent Image synthetic red swatch" }).unwrap();
    let mut resources = RasterResources::coverage8(&assets);
    let panel = [fill(SurfaceId::Osd, 0, 0, 1)];
    let wide = [fill(SurfaceId::Stage3, 959, 543, 1)];
    let batch = render_output(&mut context, &mut resources, OutputMode::PanelAndWide, DrawList::new(SurfaceId::Osd, &panel), Some(DrawList::new(SurfaceId::Stage3, &wide))).unwrap();
    assert_eq!(context.take_output_presented(), None);
    assert_eq!(context.drain_state(), DrainState::Pending);
    for frame in batch.frames().into_iter().flatten() {
        context.surface(frame.surface).unwrap().submit(frame.frame, |surface, _, pixels| {
            match surface {
                SurfaceId::Osd => { assert_eq!(&pixels[..4], &[255, 0, 0, 255]); assert_eq!(&pixels[pixels.len() - 4..], &[0; 4]); }
                SurfaceId::Stage3 => { assert_eq!(&pixels[..4], &[0; 4]); assert_eq!(&pixels[pixels.len() - 4..], &[255, 0, 0, 255]); }
                _ => panic!("unexpected output"),
            }
            Ok::<_, SubmissionError<()>> (())
        }).unwrap().unwrap();
    }
    assert_eq!(context.take_output_presented(), None);
}

#[test]
fn second_surface_asset_error_cancels_entire_output_and_allows_retry() {
    static PANEL: StaticBuffers = StaticBuffers::new(0);
    static WIDE: StaticBuffers<{ 960 * 544 * 4 }> = StaticBuffers::new(0);
    let [a, b] = PANEL.take();
    let [c, d] = WIDE.take();
    let mut context = GraphicsContext::new();
    assert!(context.install(Publisher::new(ContextId::new(22).unwrap(), SurfaceId::Osd, a, b).unwrap()).is_ok());
    assert!(context.install(Publisher::new(ContextId::new(23).unwrap(), SurfaceId::Stage3, c, d).unwrap()).is_ok());
    let assets = AssetPack::parse(PACK, AssetProvenance::Original { author: "Latent Image synthetic red swatch" }).unwrap();
    let mut resources = RasterResources::coverage8(&assets);
    let panel = [fill(SurfaceId::Osd, 0, 0, 1)];
    let wide = [fill(SurfaceId::Stage3, 0, 0, 2)];
    assert_eq!(render_output(&mut context, &mut resources, OutputMode::PanelAndWide, DrawList::new(SurfaceId::Osd, &panel), Some(DrawList::new(SurfaceId::Stage3, &wide))), Err(RenderError::Raster(RasterError::MissingPalette(PaletteIndex(2)))));
    assert_eq!(context.drain_state(), DrainState::Drained);
    assert_eq!(context.surface(SurfaceId::Osd).unwrap().queued(), None);
    assert_eq!(context.surface(SurfaceId::Stage3).unwrap().queued(), None);
    let retry = context.begin_output(OutputMode::PanelAndWide).unwrap();
    context.cancel_output(retry).unwrap();
}
