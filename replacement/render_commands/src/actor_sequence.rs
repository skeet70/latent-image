use super::actor::{ActorError, Event, TaskActor};
use super::actor_request::{CallbackBinding, QueueRequest, RenderComposition, RequestError};
use super::context::GraphicsContext;
use super::raster::RasterResources;

pub enum CompositionDescriptor<'a> {
    Open(&'a mut Option<GraphicsContext>),
    Close,
    Render(RenderComposition<'a>),
}
pub struct QueueMessage<'a> { pub operation: u16, pub payload: &'a [u8] }

pub fn consume(
    actor: &mut TaskActor,
    assets: &mut RasterResources<'_, '_, '_>,
    message: QueueMessage<'_>,
    bindings: &[CallbackBinding],
    descriptor: CompositionDescriptor<'_>,
) -> Result<Event, ActorError> {
    let request = QueueRequest::decode(message.operation, message.payload, bindings).map_err(ActorError::Request)?;
    match (request, descriptor) {
        (request @ QueueRequest::Open { .. }, CompositionDescriptor::Open(context)) => {
            let owned = context.take().ok_or(ActorError::NotOpen)?;
            match actor.open(owned, request) {
                Ok(event) => Ok(event),
                Err(failure) => { *context = Some(failure.context); Err(failure.error) }
            }
        }
        (request @ QueueRequest::Close { .. }, CompositionDescriptor::Close) => actor.close(request),
        (request @ QueueRequest::Render { .. }, CompositionDescriptor::Render(composition)) => {
            actor.render(assets, composition.resolve(request).map_err(ActorError::Request)?)
        }
        _ => Err(ActorError::Request(RequestError::DescriptorMismatch)),
    }
}
