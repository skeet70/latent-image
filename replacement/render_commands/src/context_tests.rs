use super::context::{ContextError, GraphicsContext, OutputMode, OutputPlan};
use super::publisher::{CloseState, ContextId, DrainState, Publisher, SubmissionError};
use super::test_buffers::StaticBuffers;
use super::SurfaceId;

fn publisher(surface: SurfaceId, identity: u64, buffers: [&'static mut [u8]; 2]) -> Publisher {
    let [first, second] = buffers;
    Publisher::new(ContextId::new(identity).unwrap(), surface, first, second).unwrap()
}

#[test]
fn all_twelve_buffers_close_without_egl_and_dual_output_requires_both_surfaces() {
    static S0: StaticBuffers = StaticBuffers::new(0);
    static S1: StaticBuffers = StaticBuffers::new(0);
    static S2: StaticBuffers<{ 360 * 240 * 4 }> = StaticBuffers::new(0);
    static S3: StaticBuffers<{ 960 * 544 * 4 }> = StaticBuffers::new(0);
    static S4: StaticBuffers<{ 1280 * 720 * 4 }> = StaticBuffers::new(0);
    static S5: StaticBuffers<{ 640 * 480 * 4 }> = StaticBuffers::new(0);
    let mut context = GraphicsContext::new();
    assert!(context.install(publisher(SurfaceId::Osd, 1, S0.take())).is_ok());
    assert_eq!(context.require_output(OutputMode::from_stock(8)), Err(ContextError::MissingSurface));
    assert!(context.install(publisher(SurfaceId::Stage1, 2, S1.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage2, 3, S2.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage3, 4, S3.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage4, 5, S4.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage5, 6, S5.take())).is_ok());
    assert_eq!(context.require_output(OutputMode::from_stock(8)), Ok(OutputPlan { first: SurfaceId::Osd, second: Some(SurfaceId::Stage3) }));
    assert_eq!(context.require_output(OutputMode::from_stock(10)), Ok(OutputPlan { first: SurfaceId::Osd, second: Some(SurfaceId::Stage5) }));
    assert_eq!(context.require_output(OutputMode::from_stock(6)), Ok(OutputPlan { first: SurfaceId::Stage4, second: None }));
    assert_eq!(context.close(), CloseState::Released);
    assert_eq!(context.drain_state(), DrainState::Drained);
    assert_eq!(context.require_output(OutputMode::Panel), Err(ContextError::Closed));
    for surface in [SurfaceId::Osd, SurfaceId::Stage1, SurfaceId::Stage2, SurfaceId::Stage3, SurfaceId::Stage4, SurfaceId::Stage5] {
        let [first, second] = context.release(surface).unwrap();
        assert_eq!(first.len(), surface.byte_len());
        assert_eq!(second.len(), surface.byte_len());
    }
}

#[test]
fn context_close_keeps_uncertain_transport_until_corresponding_detach() {
    static BUFFERS: StaticBuffers = StaticBuffers::new(0);
    let mut context = GraphicsContext::new();
    assert!(context.install(publisher(SurfaceId::Osd, 7, BUFFERS.take())).is_ok());
    let surface = context.surface(SurfaceId::Osd).unwrap();
    let frame = surface.begin().unwrap();
    surface.publish(frame).unwrap();
    assert_eq!(surface.submit(frame, |_, _, _| Err(SubmissionError::Uncertain(17))), Ok(Err(SubmissionError::Uncertain(17))));
    assert_eq!(context.close(), CloseState::Pending);
    assert!(context.release(SurfaceId::Osd).is_err());
    assert_eq!(context.close_state(), CloseState::Pending);
    assert!(context.acknowledge_detached(SurfaceId::Osd, ContextId::new(6).unwrap()).is_err());
    assert_eq!(context.acknowledge_detached(SurfaceId::Osd, ContextId::new(7).unwrap()), Ok(CloseState::Released));
    assert_eq!(context.close_state(), CloseState::Released);
    assert!(context.release(SurfaceId::Osd).is_ok());
}

#[test]
fn dual_output_waits_for_both_actual_acknowledgments_and_repeats_after_completion() {
    static PANEL: StaticBuffers = StaticBuffers::new(0);
    static WIDE: StaticBuffers<{ 960 * 544 * 4 }> = StaticBuffers::new(0);
    let mut context = GraphicsContext::new();
    assert!(context.install(publisher(SurfaceId::Osd, 8, PANEL.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage3, 9, WIDE.take())).is_ok());
    let batch = context.begin_output(OutputMode::PanelAndWide).unwrap();
    let [Some(first), Some(second)] = batch.frames() else { panic!("dual output omitted a surface") };
    assert_eq!(context.begin_output(OutputMode::PanelAndWide), Err(ContextError::Busy));
    context.publish_output(batch).unwrap();
    for frame in [first, second] {
        context.surface(frame.surface).unwrap().submit(frame.frame, |_, _, _| Ok::<_, SubmissionError<()>> (())).unwrap().unwrap();
    }
    assert_eq!(context.take_output_presented(), None);
    context.acknowledge(second.surface, second.frame).unwrap();
    assert_eq!(context.take_output_presented(), None);
    assert!(context.acknowledge(first.surface, second.frame).is_err());
    context.acknowledge(first.surface, first.frame).unwrap();
    assert_eq!(context.take_output_presented().unwrap().batch, batch);
    assert_eq!(context.take_output_presented(), None);
    let next = context.begin_output(OutputMode::PanelAndWide).unwrap();
    context.cancel_output(next).unwrap();
    assert_eq!(context.take_output_presented(), None);
    assert!(context.begin_output(OutputMode::PanelAndWide).is_ok());
}

#[test]
fn partial_batch_preflight_refuses_without_queuing_first_surface() {
    static PANEL: StaticBuffers = StaticBuffers::new(0);
    static STANDARD: StaticBuffers<{ 640 * 480 * 4 }> = StaticBuffers::new(0);
    let mut context = GraphicsContext::new();
    assert!(context.install(publisher(SurfaceId::Osd, 10, PANEL.take())).is_ok());
    assert!(context.install(publisher(SurfaceId::Stage5, 11, STANDARD.take())).is_ok());
    let older = context.surface(SurfaceId::Stage5).unwrap().begin().unwrap();
    let batch = context.begin_output(OutputMode::PanelAndStandard).unwrap();
    let [Some(first), Some(second)] = batch.frames() else { panic!("dual output omitted a surface") };
    assert!(context.publish_output(batch).is_err());
    assert!(context.surface(first.surface).unwrap().drawing_pixels(first.frame).is_ok());
    assert!(context.surface(second.surface).unwrap().drawing_pixels(second.frame).is_ok());
    context.surface(SurfaceId::Stage5).unwrap().cancel(older).unwrap();
    context.publish_output(batch).unwrap();
    context.cancel_output(batch).unwrap();
    assert_eq!(context.drain_state(), DrainState::Drained);
}
