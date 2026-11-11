use super::actor::{ActorError, Event, PendingReason, TaskActor, WaiterStatus};
use super::actor_request::{CallbackAddress, CallbackBinding, InitializationWord, QueueRequest, RenderComposition, RenderRequest, RequestError, WaiterId};
use super::actor_sequence::{consume, CompositionDescriptor, QueueMessage};
use super::commands::{Alpha, Colour, DrawCommand, DrawList};
use super::context::{GraphicsContext, OutputMode};
use super::publisher::{CloseState, ContextId, Publisher, SubmissionError};
use super::raster::{AssetPack, AssetProvenance, RasterResources};
use super::test_buffers::StaticBuffers;
use super::{PaletteIndex, PanelPoint, PanelRect, SurfaceId};

const PACK: &[u8] = &[
    b'R', b'C', b'P', b'1', 1, 0, 0, 0,
    1, 0, 0, 0, 1, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 4, 0, 0, 0,
    255, 0, 0, 255,
];
fn waiter(value: u32) -> WaiterId { WaiterId(core::num::NonZeroU32::new(value).unwrap()) }
fn fill(surface: SurfaceId, palette: u32) -> DrawCommand<'static> {
    DrawCommand::Fill {
        rect: PanelRect::new_for(surface, PanelPoint::new_for(surface, 0, 0).unwrap(), 1, 1).unwrap(),
        colour: Colour { palette: PaletteIndex(palette), alpha: Alpha::OPAQUE },
    }
}
#[test]
fn every_stage_and_atomic_modes_keep_pixels_and_waiters_until_real_events() {
    static S0: StaticBuffers = StaticBuffers::new(0);
    static S1: StaticBuffers = StaticBuffers::new(0);
    static S2: StaticBuffers<{ 360 * 240 * 4 }> = StaticBuffers::new(0);
    static S3: StaticBuffers<{ 960 * 544 * 4 }> = StaticBuffers::new(0);
    static S4: StaticBuffers<{ 1280 * 720 * 4 }> = StaticBuffers::new(0);
    static S5: StaticBuffers<{ 640 * 480 * 4 }> = StaticBuffers::new(0);
    let mut context = GraphicsContext::new();
    let surfaces = [SurfaceId::Osd, SurfaceId::Stage1, SurfaceId::Stage2, SurfaceId::Stage3, SurfaceId::Stage4, SurfaceId::Stage5];
    for ((index, surface), [a, b]) in surfaces.into_iter().enumerate().zip([S0.take(), S1.take(), S2.take(), S3.take(), S4.take(), S5.take()]) {
        assert!(context.install(Publisher::new(ContextId::new(100 + index as u64).unwrap(), surface, a, b).unwrap()).is_ok());
    }
    let mut actor = TaskActor::new();
    let assets = AssetPack::parse(PACK, AssetProvenance::Original { author: "Latent Image red swatch" }).unwrap();
    let mut resources = RasterResources::coverage8(&assets);
    let mut owned = Some(context);
    let bindings = [CallbackBinding { address: CallbackAddress::new(0x6dbf6c).unwrap(), waiter: waiter(1) }];
    let mut open = [0; 8];
    open[4..].copy_from_slice(&0x6dbf6cu32.to_le_bytes());
    assert!(consume(&mut actor, &mut resources, QueueMessage { operation: 0x5900, payload: &open }, &bindings, CompositionDescriptor::Open(&mut owned)).is_ok());
    assert!(owned.is_none());
    assert_eq!(actor.take_waiters().unwrap().status, WaiterStatus::Accepted);
    for mode in [OutputMode::Panel, OutputMode::PanelAlternate, OutputMode::HalfPanel, OutputMode::Wide544, OutputMode::Hd720, OutputMode::Standard480, OutputMode::PanelAndWide, OutputMode::PanelAndStandard] {
        let plan = mode.plan();
        let first = [fill(plan.first, 1)];
        let second = [fill(plan.second.unwrap_or(plan.first), 1)];
        let request = RenderRequest { composition: RenderComposition { mode, first: DrawList::new(plan.first, &first), second: plan.second.map(|surface| DrawList::new(surface, &second)) }, waiters: [Some(waiter(2)); 6] };
        let batch = actor.queue_render(&mut resources, request).unwrap();
        for frame in batch.frames().into_iter().flatten() {
            actor.submit(frame.surface, frame.frame, |_, _, pixels| {
                assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
                assert_eq!(&pixels[4..8], &[0; 4]);
                Ok::<_, SubmissionError<()>>(())
            }).unwrap().unwrap();
        }
        assert_eq!(actor.take_waiters(), None);
        for (index, frame) in batch.frames().into_iter().flatten().enumerate() {
            let event = actor.acknowledge(frame.surface, frame.frame).unwrap();
            if index == 0 && plan.second.is_some() {
                assert!(matches!(event, Event::Pending { reason: PendingReason::Presentation, .. }));
                assert_eq!(actor.take_waiters(), None);
            } else { assert!(matches!(event, Event::Accepted { .. })); }
        }
        let delivery = actor.take_waiters().unwrap();
        assert_eq!(delivery.waiters, [Some(waiter(2)); 6]);
        assert_eq!(delivery.status, WaiterStatus::Accepted);
        assert_eq!(actor.take_waiters(), None);
    }
    assert!(matches!(actor.close(QueueRequest::Close { waiter: Some(waiter(3)) }).unwrap(), Event::Pending { reason: PendingReason::Release, .. }));
    assert_eq!(actor.take_waiters(), None);
    assert!(matches!(actor.take_released_context(), Err(ActorError::Busy)));
    assert!(actor.acknowledge_detached(SurfaceId::Osd, ContextId::new(999).unwrap()).is_err());
    for (index, surface) in surfaces.into_iter().enumerate() {
        let event = actor.acknowledge_detached(surface, ContextId::new(100 + index as u64).unwrap()).unwrap();
        if index == 5 { assert!(matches!(event, Event::Closed { .. })); }
        else { assert!(matches!(event, Event::Pending { reason: PendingReason::Release, .. })); assert_eq!(actor.take_waiters(), None); }
    }
    assert_eq!(actor.take_waiters().unwrap().status, WaiterStatus::Closed);
    let mut context = actor.take_released_context().unwrap();
    assert_eq!(context.close_state(), CloseState::Released);
    for surface in surfaces { let [a, b] = context.release(surface).unwrap(); assert_eq!(a.len(), surface.byte_len()); assert_eq!(b.len(), surface.byte_len()); }
}

#[test]
fn abi_decodes_all_six_waiters_without_executing_addresses() {
    let bindings = [CallbackBinding { address: CallbackAddress::new(0x6dbf6c).unwrap(), waiter: waiter(7) }];
    let mut payload = [0; 48];
    payload[20..24].copy_from_slice(&0x6dbf6cu32.to_le_bytes());
    payload[24..].fill(0xa5);
    let QueueRequest::Render { waiters } = QueueRequest::decode(0x5902, &payload, &bindings).unwrap() else { panic!("render request"); };
    assert_eq!(waiters, [None, None, None, None, None, Some(waiter(7))]);
    assert_eq!(QueueRequest::decode(0x5902, &payload[..24], &bindings), Err(RequestError::PayloadLength));
    assert_eq!(QueueRequest::decode(0x5901, &0x6dbf6cu32.to_le_bytes(), &[]), Err(RequestError::UnresolvedCallback(bindings[0].address)));
    assert_eq!(QueueRequest::decode(0x5901, &0x6dbf6cu32.to_le_bytes(), &bindings).unwrap(), QueueRequest::Close { waiter: Some(waiter(7)) });
    let mut open = [0; 8];
    open[..4].copy_from_slice(&17u32.to_le_bytes());
    open[4..].copy_from_slice(&0x6dbf6cu32.to_le_bytes());
    assert_eq!(QueueRequest::decode(0x5900, &open, &bindings).unwrap(), QueueRequest::Open { initialization: InitializationWord(17), waiter: Some(waiter(7)) });
}

#[test]
fn partial_context_admission_preserves_atomic_output_and_close_cancels_unsubmitted_pixels() {
    static PANEL: StaticBuffers = StaticBuffers::new(0);
    let [a, b] = PANEL.take();
    let mut context = GraphicsContext::new();
    assert!(context.install(Publisher::new(ContextId::new(200).unwrap(), SurfaceId::Osd, a, b).unwrap()).is_ok());
    let mut actor = TaskActor::new();
    assert!(actor.open(context, QueueRequest::Open { initialization: InitializationWord(0), waiter: None }).is_ok());
    assert_eq!(actor.take_waiters(), None);
    let assets = AssetPack::parse(PACK, AssetProvenance::Original { author: "Latent Image red swatch" }).unwrap();
    let mut resources = RasterResources::coverage8(&assets);
    let panel = [fill(SurfaceId::Osd, 1)];
    let wide = [fill(SurfaceId::Stage3, 1)];
    let dual = RenderRequest { composition: RenderComposition { mode: OutputMode::PanelAndWide, first: DrawList::new(SurfaceId::Osd, &panel), second: Some(DrawList::new(SurfaceId::Stage3, &wide)) }, waiters: [Some(waiter(8)); 6] };
    assert_eq!(actor.render(&mut resources, dual), Err(ActorError::Render(super::runtime::RenderError::Context(super::context::ContextError::MissingSurface))));
    assert!(matches!(actor.take_waiters().unwrap().status, WaiterStatus::Error(_)));
    let queued = consume(&mut actor, &mut resources, QueueMessage { operation: 0x5902, payload: &[0; 48] }, &[], CompositionDescriptor::Render(RenderComposition { mode: OutputMode::Panel, first: DrawList::new(SurfaceId::Osd, &panel), second: None })).unwrap();
    assert!(matches!(queued, Event::Queued { .. }));
    assert_eq!(actor.take_waiters(), None);
    assert!(matches!(consume(&mut actor, &mut resources, QueueMessage { operation: 0x5901, payload: &[0; 4] }, &[], CompositionDescriptor::Close).unwrap(), Event::Closed { .. }));
    assert_eq!(actor.take_waiters(), None);
    let mut released = actor.take_released_context().unwrap();
    let [a, _] = released.release(SurfaceId::Osd).unwrap();
    assert_eq!(&a[..4], &[255, 0, 0, 255]);
}
