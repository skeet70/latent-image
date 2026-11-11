use super::context::{ContextError, GraphicsContext, OutputBatch, OutputMode};
use super::commands::DrawList;
use super::publisher::PublisherError;
use super::raster::{RasterResources, RasterError, Surface};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderError { Context(ContextError), Raster(RasterError), Publisher(PublisherError), OutputMismatch }

pub fn render_output(
    context: &mut GraphicsContext,
    assets: &mut RasterResources<'_, '_, '_>,
    mode: OutputMode,
    first: DrawList<'_>,
    second: Option<DrawList<'_>>,
) -> Result<OutputBatch, RenderError> {
    let plan = context.require_output(mode).map_err(RenderError::Context)?;
    if first.surface() != plan.first || second.map(|list| list.surface()) != plan.second { return Err(RenderError::OutputMismatch); }
    let batch = context.begin_output(mode).map_err(RenderError::Context)?;
    let result = (|| {
        for (frame, list) in batch.frames().into_iter().flatten().zip([Some(first), second].into_iter().flatten()) {
            let publisher = context.surface(frame.surface).map_err(RenderError::Context)?;
            let pixels = publisher.drawing_pixels(frame.frame).map_err(RenderError::Publisher)?;
            Surface::new(frame.surface, pixels).map_err(RenderError::Raster)?.render(list, assets).map_err(RenderError::Raster)?;
        }
        context.publish_output(batch).map_err(RenderError::Context)
    })();
    if result.is_err() { context.cancel_output(batch).map_err(RenderError::Context)?; }
    result
}
