use crate::render_commands::actor::{ActorError, TaskActor};
use crate::render_commands::actor_request::{RenderComposition, RenderRequest};
use crate::render_commands::raster::RasterResources;
use crate::render_commands::commands::{DrawCommand, DrawList};
use crate::render_commands::producer::{CommandBuffer, MenuView, ProducerError};
use crate::render_commands::context::{OutputBatch, OutputMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    Producer(ProducerError),
    Actor(ActorError),
}

impl From<ProducerError> for FrameError {
    fn from(error: ProducerError) -> Self { Self::Producer(error) }
}

pub(super) fn queue_panel(actor: &mut TaskActor, assets: &mut RasterResources<'_, '_, '_>, first: DrawList<'_>) -> Result<OutputBatch, FrameError> {
    actor.queue_render(assets, RenderRequest { composition: RenderComposition { mode: OutputMode::Panel, first, second: None }, waiters: [None; 6] }).map_err(FrameError::Actor)
}

pub fn compose_menu_and_publish<'assets>(
    view: MenuView<'_, 'assets>,
    commands: &mut [DrawCommand<'assets>],
    assets: &mut RasterResources<'_, '_, '_>,
    actor: &mut TaskActor,
) -> Result<OutputBatch, FrameError> {
    let mut buffer = CommandBuffer::new(commands);
    buffer.compose_menu(view).map_err(FrameError::Producer)?;
    queue_panel(actor, assets, buffer.draw_list())
}
