#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! saclac's peer ports: each request goes to its peer's queue through the kernel with the header
//! its stock sender writes (`xm1_saclac_interface::stock::peers`).

use core::num::NonZeroU8;

use xm1_kernel::{Backend, Header, InterruptContext, MessageQueue, PostError, QueueId, TaskContext};
use xm1_saclac_interface::stock::peers::{self, PeerRoute};
use xm1_saclac_interface::{
    lens_effect, Calc, Calc1, Calc1Request, CalcRequest, Flash, FlashRequest, ImageProcessor, ImageProcessorRequest, ImgDc,
    ImgDcRequest, LensCom, LensComRequest, LensDc, LensDcRequest, LensEffect, Parameters, PeerOperation, Statistics,
    StatisticsRequest,
};

#[cfg(test)]
mod tests;

/// The context a request is posted from.
pub trait Poster {
    /// What 0x723cb8 returns in this context.
    fn running_task(&self) -> Option<NonZeroU8>;
    fn post(&self, queue: MessageQueue, header: Header, payload: &[u8]) -> Result<(), PostError>;
}

impl<B: Backend> Poster for TaskContext<'_, B> {
    fn running_task(&self) -> Option<NonZeroU8> { NonZeroU8::new(self.id().get()) }

    fn post(&self, queue: MessageQueue, header: Header, payload: &[u8]) -> Result<(), PostError> {
        queue.post(self, header, payload)
    }
}

impl<B: Backend> Poster for InterruptContext<'_, B> {
    fn running_task(&self) -> Option<NonZeroU8> { None }

    fn post(&self, queue: MessageQueue, header: Header, payload: &[u8]) -> Result<(), PostError> {
        queue.post(self, header, payload)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Which requests a port posts, by the effect `lens_effect` gives them (decision Q9).
pub trait LensAccess: sealed::Sealed {
    fn admits(&self, effect: LensEffect) -> bool;
}

/// Posts only requests that cannot reach a lens drive.
#[derive(Clone, Copy, Debug)]
pub struct LensDriveRefused;

/// Posts every request, lens-reaching ones included.
#[derive(Clone, Copy, Debug)]
pub struct EmulatorOnly(());

impl EmulatorOnly {
    /// `marker` is the image's `XM1_REQUIRES_LENS_SAFETY_CONTRACT`, from which the fixed-image
    /// stager marks the image `emulator_only`.
    pub fn new(marker: &'static u32) -> Option<Self> { (*marker == 1).then_some(Self(())) }
}

impl sealed::Sealed for LensDriveRefused {}
impl sealed::Sealed for EmulatorOnly {}

impl LensAccess for LensDriveRefused {
    fn admits(&self, effect: LensEffect) -> bool { !effect.requires_lens_safety_contract() }
}

impl LensAccess for EmulatorOnly {
    fn admits(&self, _: LensEffect) -> bool { true }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerError {
    /// Not posted: the request can reach a lens drive and the port is `LensDriveRefused`.
    LensSafetyContract(LensEffect),
    /// The kernel refused the post and reported it as 0x72503c does.
    Refused(PostError),
}

pub struct PeerPorts<'c, P, L> {
    poster: &'c P,
    lens: L,
}

impl<'c, P: Poster, L: LensAccess> PeerPorts<'c, P, L> {
    pub fn new(poster: &'c P, lens: L) -> Self { Self { poster, lens } }

    fn post(&self, operation: PeerOperation, parameters: Parameters<'_>) -> Result<(), PeerError> {
        let effect = lens_effect(operation);
        if !self.lens.admits(effect) {
            return Err(PeerError::LensSafetyContract(effect));
        }
        let route = peers::route(operation);
        let header = Header {
            operation: peers::operation_code(operation),
            sender: route.sender.byte(self.poster.running_task()),
            destination: route.queue.number(),
        };
        self.poster.post(kernel_queue(operation), header, parameters.bytes()).map_err(PeerError::Refused)
    }
}

const fn stock_queue(route: PeerRoute) -> MessageQueue {
    match QueueId::new(route.queue.number()) {
        Some(id) => MessageQueue::stock(id),
        None => panic!("peer queue outside the kernel's range"),
    }
}

fn kernel_queue(operation: PeerOperation) -> MessageQueue {
    match operation {
        PeerOperation::LensDc(_) => const { stock_queue(peers::LENS_DC) },
        PeerOperation::ImgDc(_) => const { stock_queue(peers::IMG_DC) },
        PeerOperation::Calc(_) => const { stock_queue(peers::CALC) },
        PeerOperation::Calc1(_) => const { stock_queue(peers::CALC1) },
        PeerOperation::Statistics(_) => const { stock_queue(peers::STATISTICS) },
        PeerOperation::Flash(_) => const { stock_queue(peers::FLASH) },
        PeerOperation::LensCom(_) => const { stock_queue(peers::LENS_COM) },
        PeerOperation::ImageProcessor(_) => const { stock_queue(peers::IMAGE_PROCESSOR) },
    }
}

macro_rules! port {
    ($port:ident, $request:ident) => {
        impl<P: Poster, L: LensAccess> $port for PeerPorts<'_, P, L> {
            type Error = PeerError;

            fn request(&mut self, request: $request<'_>) -> Result<(), PeerError> {
                self.post(PeerOperation::$port(request.operation), request.parameters)
            }
        }
    };
}

port!(LensDc, LensDcRequest);
port!(ImgDc, ImgDcRequest);
port!(Calc, CalcRequest);
port!(Calc1, Calc1Request);
port!(Statistics, StatisticsRequest);
port!(Flash, FlashRequest);
port!(LensCom, LensComRequest);
port!(ImageProcessor, ImageProcessorRequest);
