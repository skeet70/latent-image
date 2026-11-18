#![forbid(unsafe_code)]

use core::num::NonZeroU8;
use xm1_kernel::{Backend, Context, Header, Kernel, MessageQueue, PostError};

/// A persistent poster; context proofs exist only during each kernel operation.
pub struct Poster<B: Backend> { kernel: Kernel<B> }
impl<B: Backend> Poster<B> {
    pub const fn new(backend: B) -> Self { Self { kernel: Kernel::new(backend) } }
    pub fn kernel(&self) -> &Kernel<B> { &self.kernel }
}
impl<B: Backend> xm1_saclac_peers::Poster for Poster<B> {
    fn running_task(&self) -> Option<NonZeroU8> {
        match self.kernel.context() {
            Context::Task(context) => NonZeroU8::new(context.id().get()),
            Context::Interrupt(_) | Context::Masked(_) => None,
        }
    }
    fn post(&self, queue: MessageQueue, header: Header, payload: &[u8]) -> Result<(), PostError> {
        match self.kernel.context() {
            Context::Task(context) => queue.post(&context, header, payload),
            Context::Interrupt(context) => queue.post(&context, header, payload),
            Context::Masked(_) => panic!("peer post requires task or interrupt context"),
        }
    }
}
