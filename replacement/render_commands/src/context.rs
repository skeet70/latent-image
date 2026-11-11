use super::publisher::{CloseState, ContextId, DrainState, FrameId, FramePresented, Publisher, PublisherError};
use super::SurfaceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode { Panel, PanelAlternate, HalfPanel, Wide544, Hd720, Standard480, PanelAndWide, PanelAndStandard }
impl OutputMode {
    pub const fn from_stock(value: u32) -> Self {
        match value {
            2 => Self::PanelAlternate,
            3 => Self::HalfPanel,
            5 => Self::Wide544,
            6 => Self::Hd720,
            7 => Self::Standard480,
            8 => Self::PanelAndWide,
            10 => Self::PanelAndStandard,
            _ => Self::Panel,
        }
    }
    pub const fn plan(self) -> OutputPlan {
        match self {
            Self::Panel => OutputPlan { first: SurfaceId::Osd, second: None },
            Self::PanelAlternate => OutputPlan { first: SurfaceId::Stage1, second: None },
            Self::HalfPanel => OutputPlan { first: SurfaceId::Stage2, second: None },
            Self::Wide544 => OutputPlan { first: SurfaceId::Stage3, second: None },
            Self::Hd720 => OutputPlan { first: SurfaceId::Stage4, second: None },
            Self::Standard480 => OutputPlan { first: SurfaceId::Stage5, second: None },
            Self::PanelAndWide => OutputPlan { first: SurfaceId::Osd, second: Some(SurfaceId::Stage3) },
            Self::PanelAndStandard => OutputPlan { first: SurfaceId::Osd, second: Some(SurfaceId::Stage5) },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputPlan { pub first: SurfaceId, pub second: Option<SurfaceId> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextError { Closed, Busy, UnknownBatch, SurfaceAlreadyOwned, DuplicateIdentity, MissingSurface, Publisher(PublisherError) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceFrame { pub surface: SurfaceId, pub frame: FrameId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputBatch { first: SurfaceFrame, second: Option<SurfaceFrame> }
impl OutputBatch {
    pub const fn frames(self) -> [Option<SurfaceFrame>; 2] { [Some(self.first), self.second] }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputPresented { pub batch: OutputBatch }
struct PendingOutput { batch: OutputBatch, acknowledged: [bool; 2] }

pub struct GraphicsContext { surfaces: [Option<Publisher>; 6], closed: bool, pending: Option<PendingOutput> }
impl GraphicsContext {
    pub const fn new() -> Self { Self { surfaces: [None, None, None, None, None, None], closed: false, pending: None } }

    pub fn install(&mut self, publisher: Publisher) -> Result<(), (ContextError, Publisher)> {
        if self.closed { return Err((ContextError::Closed, publisher)); }
        let index = slot(publisher.surface());
        if self.surfaces[index].is_some() { return Err((ContextError::SurfaceAlreadyOwned, publisher)); }
        if self.surfaces.iter().flatten().any(|other| other.context() == publisher.context()) { return Err((ContextError::DuplicateIdentity, publisher)); }
        self.surfaces[index] = Some(publisher);
        Ok(())
    }

    pub fn surface(&mut self, surface: SurfaceId) -> Result<&mut Publisher, ContextError> {
        self.surfaces[slot(surface)].as_mut().ok_or(ContextError::MissingSurface)
    }

    pub fn require_output(&self, mode: OutputMode) -> Result<OutputPlan, ContextError> {
        if self.closed { return Err(ContextError::Closed); }
        let plan = mode.plan();
        if self.surfaces[slot(plan.first)].is_none() || plan.second.is_some_and(|surface| self.surfaces[slot(surface)].is_none()) { return Err(ContextError::MissingSurface); }
        Ok(plan)
    }

    pub fn begin_output(&mut self, mode: OutputMode) -> Result<OutputBatch, ContextError> {
        if self.pending.is_some() { return Err(ContextError::Busy); }
        let plan = self.require_output(mode)?;
        let first = self.surface(plan.first)?.begin().map_err(ContextError::Publisher)?;
        let second = if let Some(surface) = plan.second {
            match self.surface(surface)?.begin() {
                Ok(frame) => Some(SurfaceFrame { surface, frame }),
                Err(error) => {
                    self.surface(plan.first)?.cancel(first).map_err(ContextError::Publisher)?;
                    return Err(ContextError::Publisher(error));
                }
            }
        } else { None };
        let batch = OutputBatch { first: SurfaceFrame { surface: plan.first, frame: first }, second };
        self.pending = Some(PendingOutput { batch, acknowledged: [false, second.is_none()] });
        Ok(batch)
    }

    pub fn publish_output(&mut self, batch: OutputBatch) -> Result<OutputBatch, ContextError> {
        self.validate_batch(batch)?;
        for frame in batch.frames().into_iter().flatten() {
            self.surface(frame.surface)?.validate_publish(frame.frame).map_err(ContextError::Publisher)?;
        }
        for frame in batch.frames().into_iter().flatten() {
            self.surface(frame.surface)?.publish(frame.frame).map_err(ContextError::Publisher)?;
        }
        Ok(batch)
    }

    pub fn cancel_output(&mut self, batch: OutputBatch) -> Result<(), ContextError> {
        self.validate_batch(batch)?;
        for frame in batch.frames().into_iter().flatten() {
            self.surface(frame.surface)?.validate_cancel(frame.frame).map_err(ContextError::Publisher)?;
        }
        for frame in batch.frames().into_iter().flatten() {
            self.surface(frame.surface)?.cancel(frame.frame).map_err(ContextError::Publisher)?;
        }
        self.pending = None;
        Ok(())
    }

    pub fn take_output_presented(&mut self) -> Option<OutputPresented> {
        if self.pending.as_ref().is_some_and(|pending| pending.acknowledged == [true, true]) {
            self.pending.take().map(|pending| OutputPresented { batch: pending.batch })
        } else { None }
    }

    fn validate_batch(&self, batch: OutputBatch) -> Result<(), ContextError> {
        if self.closed { return Err(ContextError::Closed); }
        if !self.pending.as_ref().is_some_and(|pending| pending.batch == batch) { return Err(ContextError::UnknownBatch); }
        Ok(())
    }

    pub fn acknowledge(&mut self, surface: SurfaceId, frame: FrameId) -> Result<FramePresented, ContextError> {
        let presented = self.surface(surface)?.acknowledge(frame).map_err(ContextError::Publisher)?;
        if let Some(pending) = &mut self.pending {
            for (index, expected) in pending.batch.frames().into_iter().enumerate() {
                if expected == Some(SurfaceFrame { surface, frame }) { pending.acknowledged[index] = true; }
            }
        }
        Ok(presented)
    }

    pub fn acknowledge_detached(&mut self, surface: SurfaceId, context: ContextId) -> Result<CloseState, ContextError> {
        self.surface(surface)?.acknowledge_detached(context).map_err(ContextError::Publisher)
    }

    pub fn drain_state(&self) -> DrainState {
        if self.surfaces.iter().flatten().any(|surface| surface.drain_state() == DrainState::Pending) { DrainState::Pending } else { DrainState::Drained }
    }

    pub fn close(&mut self) -> CloseState {
        self.closed = true;
        self.pending = None;
        for surface in self.surfaces.iter_mut().flatten() { surface.close(); }
        self.close_state()
    }

    pub fn close_state(&self) -> CloseState {
        if self.closed && self.surfaces.iter().flatten().all(|surface| surface.close_state() == CloseState::Released) { CloseState::Released } else { CloseState::Pending }
    }

    pub fn release(&mut self, surface: SurfaceId) -> Result<[&'static mut [u8]; 2], ContextError> {
        let index = slot(surface);
        let publisher = self.surfaces[index].take().ok_or(ContextError::MissingSurface)?;
        match publisher.release() {
            Ok(buffers) => Ok(buffers),
            Err(publisher) => { self.surfaces[index] = Some(publisher); Err(ContextError::Publisher(PublisherError::Busy)) }
        }
    }
}
impl Default for GraphicsContext { fn default() -> Self { Self::new() } }
const fn slot(surface: SurfaceId) -> usize {
    match surface { SurfaceId::Osd => 0, SurfaceId::Stage1 => 1, SurfaceId::Stage2 => 2, SurfaceId::Stage3 => 3, SurfaceId::Stage4 => 4, SurfaceId::Stage5 => 5 }
}
