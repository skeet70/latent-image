#![forbid(unsafe_code)]

use xm1_kernel::{Backend, Context, FlagBits, TaskFlags, TaskId, Timeout};
use xm1_saclac_interface::{lens_effect, Calc, Calc1, Calc1Request, CalcRequest, CalcSub, CalcSubRequest, Flash, FlashRequest, ImageProcessor, ImageProcessorRequest, ImgDc, ImgDcRequest, LensCom, LensComRequest, LensDc, LensDcOperation, LensDcRequest, PeerOperation, Statistics, StatisticsRequest, stock::{parameters, StockCallbackWord}};
use xm1_saclac_peers::{LensAccess, PeerError, PeerPorts};
use xm1_saclac_shared::stock::Machine;
use crate::peer_context::Poster;

/// The two rendezvous actually produced by the shot sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rendezvous { Adjustment, Lens }
impl Rendezvous {
    const fn offset(self) -> u32 { match self { Self::Adjustment => 4, Self::Lens => 0 } }
    const fn flag(self) -> FlagBits { FlagBits(match self { Self::Adjustment => 4, Self::Lens => 2 }) }
}
#[derive(Clone, Copy)]
pub struct Callbacks { pub adjustment: StockCallbackWord, pub lens: StockCallbackWord, pub subject: StockCallbackWord }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Context,
    InvalidWaiter(u32),
    Kernel(xm1_kernel::Error),
    Peer(PeerError),
}

/// Handles to the same peer queues and shared waiter words, with no retained context proof.
pub struct Peers<'a, M, B: Backend, L> {
    poster: &'a Poster<B>,
    machine: M,
    lens: L,
    callbacks: Callbacks,
}
impl<'a, M, B: Backend, L> Peers<'a, M, B, L> {
    pub const fn new(poster: &'a Poster<B>, machine: M, lens: L, callbacks: Callbacks) -> Self {
        Self { poster, machine, lens, callbacks }
    }
}

macro_rules! peer {
    ($contract:ident, $request:ident) => {
        impl<M, B: Backend, L: LensAccess + Copy> $contract for Peers<'_, M, B, L> {
            type Error = PeerError;
            fn request(&mut self, request: $request<'_>) -> Result<(), PeerError> {
                $contract::request(&mut PeerPorts::new(self.poster, self.lens), request)
            }
        }
    };
}
peer!(Calc, CalcRequest);
peer!(Calc1, Calc1Request);
peer!(CalcSub, CalcSubRequest);
peer!(Flash, FlashRequest);
peer!(ImageProcessor, ImageProcessorRequest);
peer!(ImgDc, ImgDcRequest);
peer!(LensCom, LensComRequest);
peer!(LensDc, LensDcRequest);
peer!(Statistics, StatisticsRequest);

impl<M: Machine, B: Backend, L: LensAccess + Copy> Peers<'_, M, B, L> {
    /// f4727c/f472bc: publish the waiter before requesting, then hold task 43 for its flag.
    /// The native wait's mode word is 1, the same mode the kernel's wait_any uses.
    pub fn synchronize(&mut self, kind: Rendezvous) -> Result<(), Error> {
        let context = match self.poster.kernel().context() { Context::Task(context) => context, _ => return Err(Error::Context) };
        if kind == Rendezvous::Lens {
            let effect = lens_effect(PeerOperation::LensDc(LensDcOperation::Request17));
            if !self.lens.admits(effect) { return Err(Error::Peer(PeerError::LensSafetyContract(effect))); }
        }
        self.machine.write_u32(crate::stock::RENDEZVOUS_WAITERS + kind.offset(), u32::from(context.id().get()));
        match kind {
            Rendezvous::Adjustment => { self.machine.enter(crate::stock::REQUEST_ADJUSTMENT_SYNCHRONIZATION, &[self.callbacks.adjustment.0 as usize]); }
            Rendezvous::Lens => {
                // 76f678..76f6b4 clears all eight bytes, stores the callback word and byte 0.
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&self.callbacks.lens.0.to_le_bytes());
                LensDc::request(&mut PeerPorts::new(self.poster, self.lens), LensDcRequest { operation: LensDcOperation::Request17, parameters: parameters(&bytes), completions: &[] }).map_err(Error::Peer)?;
            }
        }
        context.wait_any(kind.flag(), Timeout::Forever).map(|_| ()).map_err(Error::Kernel)
    }
}

impl<M: Machine, B: Backend, L: LensAccess + Copy> xm1_saclac_interface::Blocking for Peers<'_, M, B, L> {
    type Error = Error;
    fn execute(&mut self, request: xm1_saclac_interface::BlockingRequest) -> Result<(), Self::Error> {
        use xm1_saclac_interface::BlockingRequest;
        self.synchronize(match request {
            BlockingRequest::AdjustmentSynchronize => Rendezvous::Adjustment,
            BlockingRequest::LensSynchronize => Rendezvous::Lens,
        })
    }
}

pub trait SubjectSelection {
    fn select_subjects(&mut self, kind: u32, option: u32) -> Result<bool, Error>;
}
impl<M: Machine, B: Backend, L> SubjectSelection for Peers<'_, M, B, L> {
    fn select_subjects(&mut self, kind: u32, option: u32) -> Result<bool, Error> {
        let context = match self.poster.kernel().context() { Context::Task(context) => context, _ => return Err(Error::Context) };
        self.machine.write_u32(crate::stock::SUBJECT_SELECTION_WAITER, u32::from(context.id().get()));
        let status = self.machine.enter(crate::stock::START_SUBJECT_SELECTION, &[kind as usize, 0, option as usize, self.callbacks.subject.0 as usize]);
        // f15800..f15820 waits and clears even when the start returned a refusal.
        context.wait_any(FlagBits(1), Timeout::Forever).map_err(Error::Kernel)?;
        context.clear_flags().map_err(Error::Kernel)?;
        Ok(status == 0)
    }
}

impl<M: Machine, B: Backend, L> xm1_shot_sequence::boundary::Completions for Peers<'_, M, B, L> {
    fn self_complete(&mut self, completion: xm1_saclac_interface::Completion) {
        let result = if completion == xm1_saclac_interface::Completion::Calculation8f {
            crate::completion_callbacks::exposure_correction(&self.machine, self.poster, None)
        } else {
            crate::completion_callbacks::post(self.poster, completion, &[])
        };
        assert!(result.is_ok(), "saclac self completion enqueue failed");
    }
}

/// f46be0/f46c00 signal the registered waiter, not the callback's running task.
pub fn signal<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, kind: Rendezvous) -> Result<(), Error> {
    signal_waiter(machine, poster, crate::stock::RENDEZVOUS_WAITERS + kind.offset(), kind.flag())
}

/// f156b4 signals the task registered by the subject selection operation.
pub fn subject_selected<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>) -> Result<(), Error> {
    signal_waiter(machine, poster, crate::stock::SUBJECT_SELECTION_WAITER, FlagBits(1))
}

fn signal_waiter<M: Machine, B: Backend>(machine: &M, poster: &Poster<B>, address: u32, flag: FlagBits) -> Result<(), Error> {
    let word = machine.read_u32(address);
    let task = u8::try_from(word).ok().and_then(TaskId::new).ok_or(Error::InvalidWaiter(word))?;
    let target = TaskFlags::stock(task);
    match poster.kernel().context() {
        Context::Task(context) => target.set(&context, flag),
        Context::Interrupt(context) => target.set(&context, flag),
        Context::Masked(_) => return Err(Error::Context),
    }.map_err(Error::Kernel)
}
