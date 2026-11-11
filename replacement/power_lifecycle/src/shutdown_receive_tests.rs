use super::*;
struct Kernel { result: Result<(), KernelStatus>, header: ReceivedHeader, length: u32, copied: Option<(PayloadAddress, u32)>, released: bool, error: Option<KernelStatus> }
impl ReceiveKernel for Kernel {
    type Block = ();
    fn receive(&mut self, _: QueueId, _: ReceiveTimeout) -> Result<(), KernelStatus> { self.result }
    fn header(&self, _: ()) -> (ReceivedHeader, u32) { (self.header, self.length) }
    unsafe fn copy_payload(&mut self, _: (), destination: PayloadAddress, length: u32) { self.copied = Some((destination, length)); }
    fn release(&mut self, _: QueueId, _: ()) { self.released = true; }
    fn report(&mut self, _: QueueId, status: KernelStatus) { self.error = Some(status); }
}
fn kernel() -> Kernel { Kernel { result: Ok(()), header: ReceivedHeader { operation: RequestId(0x1600), sender: TaskId(9), destination: QueueId(6) }, length: 4, copied: None, released: false, error: None } }
#[test]
fn payload_is_copied_to_callers_storage_before_block_release() {
    let mut kernel = kernel();
    let mut descriptor = ReceiveDescriptor::new(PayloadAddress(0x100));
    assert_eq!(unsafe { receive_into(&mut kernel, QueueId(6), &mut descriptor, ReceiveTimeout::Forever) }, Ok(()));
    assert_eq!(descriptor.header(), kernel.header);
    assert_eq!(descriptor.length(), 4);
    assert_eq!(descriptor.payload, PayloadAddress(0x100));
    assert_eq!(kernel.copied, Some((PayloadAddress(0x100), 4)));
    assert!(kernel.released);
}
#[test]
fn failed_receive_preserves_previous_message_and_skips_release() {
    let mut kernel = kernel();
    let mut descriptor = ReceiveDescriptor::new(PayloadAddress(0x100));
    unsafe { receive_into(&mut kernel, QueueId(6), &mut descriptor, ReceiveTimeout::Forever) }.unwrap();
    let previous = descriptor;
    kernel.result = Err(KernelStatus(-50));
    kernel.released = false;
    assert_eq!(unsafe { receive_into(&mut kernel, QueueId(6), &mut descriptor, ReceiveTimeout::Forever) }, Err(KernelStatus(-50)));
    assert_eq!(descriptor, previous);
    assert!(!kernel.released);
    assert_eq!(kernel.error, None);
    kernel.result = Err(KernelStatus(-18));
    let _ = unsafe { receive_into(&mut kernel, QueueId(6), &mut descriptor, ReceiveTimeout::Forever) };
    assert_eq!(kernel.error, Some(KernelStatus(-18)));
}
#[test]
fn empty_message_keeps_storage_and_releases_block() {
    let mut kernel = kernel();
    kernel.length = 0;
    let mut descriptor = ReceiveDescriptor::new(PayloadAddress(0));
    unsafe { receive_into(&mut kernel, QueueId(6), &mut descriptor, ReceiveTimeout::Forever) }.unwrap();
    assert_eq!(kernel.copied, None);
    assert!(kernel.released);
    assert_eq!(descriptor.length(), 0);
}
