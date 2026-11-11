use crate::record_address_space::{ByteCount, CallbackId, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_job_memory::{self as transport, JobError, RecordJobEffects};

pub trait ScanCacheEffects {
    fn clean_invalidate_range(&mut self, address: GuestAddress, length: ByteCount);
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Command {
    Scan = 0x3c03, Configure = 0x3c04, Measure = 0x3c05,
    Selected = 0x3c09, SelectedSmall = 0x3c0a, Extended = 0x3c0c,
    Range = 0x3c0e, Geometry = 0x3c0f, Summary = 0x3c11,
    Profile = 0x3d00, ProfilePointer = 0x3d01, Parameters = 0x3d02,
}
fn at(address: GuestAddress, offset: u32) -> Result<GuestAddress, MemoryError> { address.checked_add(offset) }
fn copy<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, destination: GuestAddress, offset: u32, source: GuestAddress, count: u32) -> Result<(), MemoryError> {
    context.copy_bytes(at(destination, offset)?, source, ByteCount(count))
}
fn post<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, command: Command, payload: GuestAddress, length: u32, callback: CallbackId) -> Result<(), JobError> {
    if matches!(command, Command::Profile | Command::ProfilePointer | Command::Parameters) {
        transport::helper_f49c18(context, command as u32, payload, ByteCount(length), &[callback])
    } else {
        transport::helper_f49598(context, command as u32, payload, ByteCount(length), &[callback])
    }
}
fn contiguous<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress, command: Command, count: u32, prefix: bool, null_status: u32) -> Result<u32, JobError> {
    if source.get() == 0 { return Ok(null_status); }
    copy(context, scratch, if prefix { 4 } else { 0 }, source, count)?;
    context.write_u32(at(scratch, if prefix { 0 } else { count })?, callback.key().get())?;
    post(context, command, scratch, count + 4, callback)?;
    Ok(0)
}

pub fn helper_f4c400<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::Selected, 0x78, true, 0x86)
}
pub fn helper_f4c448<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::SelectedSmall, 0x14, true, 0x86)
}
pub fn helper_f4c490<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::Parameters, 0x7c, false, 0x86)
}
pub fn helper_f4c4d8<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    if source.get() == 0 { return Ok(0x86); }
    context.write_u32(scratch, callback.key().get())?;
    copy(context, scratch, 4, source, 14)?;
    copy(context, scratch, 20, at(source, 40)?, 4)?;
    copy(context, scratch, 24, at(source, 14)?, 2)?;
    copy(context, scratch, 28, at(source, 16)?, 24)?;
    copy(context, scratch, 52, at(source, 44)?, 4)?;
    post(context, Command::Summary, scratch, 0x38, callback)?;
    Ok(0)
}
pub fn helper_f4c584<M: GuestAddressSpace, E: RecordJobEffects + ScanCacheEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, workspace: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    if source.get() != 0 {
        copy(context, scratch, 0, source, 0xa8)?;
        context.write_u32(at(scratch, 0xa8)?, callback.key().get())?;
    }
    let published = GuestAddress::new(0x016d4280);
    context.copy_bytes(published, workspace, ByteCount(0x4d4))?;
    context.effects.clean_invalidate_range(published, ByteCount(0x4c0));
    if source.get() == 0 { return Ok(0x86); }
    post(context, Command::Profile, scratch, 0xac, callback)?;
    Ok(0)
}
pub fn helper_f4c620<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::ProfilePointer, 0xe4, false, 0x86)
}
pub fn helper_f4c6e8<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::Extended, 0x4c, false, 0x86)
}
pub fn helper_f4c7d0<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, first: GuestAddress, second: GuestAddress, scratch: GuestAddress) -> Result<(), JobError> {
    context.write_u32(scratch, callback.key().get())?;
    copy(context, scratch, 4, first, 0x3c)?;
    copy(context, scratch, 0x40, second, 0x18)?;
    post(context, Command::Scan, scratch, 0xe8, callback)
}
pub fn helper_f4c82c<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::Configure, 0x40, true, 0x84)
}
pub fn helper_f4c88c<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>) -> Result<u32, JobError> {
    transport::helper_f49598(context, 0x3c12, GuestAddress::new(0), ByteCount(0), &[])?;
    Ok(0)
}
pub fn helper_f4c8ac<M: GuestAddressSpace, E: RecordJobEffects>(context: &mut RecordContext<M, E>, callback: CallbackId, source: GuestAddress, scratch: GuestAddress) -> Result<u32, JobError> {
    contiguous(context, callback, source, scratch, Command::Measure, 0x74, true, 0x84)
}

pub fn helper_f4f16c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32, _selector: u32) -> Result<u32, MemoryError> {
    let value = crate::record_profile_slotfields::helper_752714(context, resource)?;
    crate::record_configuration::helper_69f8fc(context, 0x2c0)?;
    Ok(value)
}
pub fn helper_f51b94<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, resource: u32, selector: u32) -> Result<u32, MemoryError> {
    let mut value = helper_f4f16c(context, resource, selector)?;
    if crate::record_profile_accessors::helper_74e7a0(context, resource)? == 2 {
        value = value.wrapping_add(crate::record_configuration::helper_69f8fc(context, 0x48a3)?);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{CallbackHandler, CallbackKey, CallbackRegistry, GuestDomain, HostAddressSpace, MemoryRegion, RecordError, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::record_job_memory::JobEnvelope;

    struct Captured {
        envelope: Option<JobEnvelope>,
        payload: [u8; 232],
        callbacks: [Option<CallbackId>; 2],
        cache: Option<(GuestAddress, ByteCount)>,
    }
    impl Captured {
        fn new() -> Self { Self { envelope: None, payload: [0; 232], callbacks: [None; 2], cache: None } }
    }
    impl RecordJobEffects for Captured {
        fn current_task(&mut self) -> u32 { 43 }
        fn submit_guest<M: GuestAddressSpace>(&mut self, memory: &mut M, envelope: JobEnvelope, source: GuestAddress, callbacks: &[CallbackId]) -> Result<(), JobError> {
            for index in 0..envelope.length.0 { self.payload[index as usize] = memory.read_u8(source.checked_add(index)?)?; }
            for (target, callback) in self.callbacks.iter_mut().zip(callbacks) { *target = Some(*callback); }
            self.envelope = Some(envelope);
            Ok(())
        }
        fn submit_owned(&mut self, envelope: JobEnvelope, source: &[u8], callbacks: &[CallbackId]) -> Result<(), JobError> {
            self.payload[..source.len()].copy_from_slice(source);
            for (target, callback) in self.callbacks.iter_mut().zip(callbacks) { *target = Some(*callback); }
            self.envelope = Some(envelope);
            Ok(())
        }
        fn invalidate(&mut self, _address: GuestAddress, _length: ByteCount) {}
        fn copy_profile_diagnostics(&mut self, _kind: u32, _source: GuestAddress, _length: ByteCount, _flags: u32) -> Result<(), JobError> { Ok(()) }
    }
    impl ScanCacheEffects for Captured {
        fn clean_invalidate_range(&mut self, address: GuestAddress, length: ByteCount) { self.cache = Some((address, length)); }
    }
    fn descriptor(base: u32, length: u32) -> RegionDescriptor {
        RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
    }
    fn callback() -> CallbackId {
        fn completion(_context: &mut (), _status: u32) -> Result<(), RecordError> { Ok(()) }
        CallbackRegistry::<(), 1>::new().register(CallbackKey::new(0x00f38668).unwrap(), CallbackHandler::Status(completion)).unwrap()
    }

    #[test]
    fn summary_submission_owns_fields_padding_and_callback_generation() {
        let mut source = [0u8; 48];
        for (index, value) in source.iter_mut().enumerate() { *value = index as u8; }
        let mut scratch = [0xa5u8; 56];
        let mut destination = 44u32.to_le_bytes();
        let mut regions = [
            MemoryRegion::new(descriptor(0x1000, 48), &mut source).unwrap(),
            MemoryRegion::new(descriptor(0x2000, 56), &mut scratch).unwrap(),
            MemoryRegion::new(descriptor(0x016c236c, 4), &mut destination).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, Captured::new());
        let callback = callback();
        assert_eq!(helper_f4c4d8(&mut context, callback, GuestAddress::new(0x1000), GuestAddress::new(0x2000)), Ok(0));
        for index in 0..56 { context.write_u8(GuestAddress::new(0x2000 + index), 0).unwrap(); }
        let packet = &context.effects.payload;
        assert_eq!(&packet[..4], &0x00f38668u32.to_le_bytes());
        assert_eq!(&packet[4..18], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        assert_eq!(&packet[18..20], &[0xa5; 2]);
        assert_eq!(&packet[20..26], &[40, 41, 42, 43, 14, 15]);
        assert_eq!(&packet[26..28], &[0xa5; 2]);
        assert_eq!(&packet[28..56], &[16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 44, 45, 46, 47]);
        assert_eq!(context.effects.callbacks[0], Some(callback));
        assert_eq!(context.effects.envelope.unwrap(), JobEnvelope { operation: 0x3c11, source: 43, destination: 44, length: ByteCount(56) });
    }

    #[test]
    fn null_profile_request_still_publishes_and_cleans_workspace_without_queueing() {
        let mut source = [0x63u8; 0x4d4];
        let mut published = [0xa5u8; 0x4d4];
        let mut regions = [
            MemoryRegion::new(descriptor(0x1000, 0x4d4), &mut source).unwrap(),
            MemoryRegion::new(descriptor(0x016d4280, 0x4d4), &mut published).unwrap(),
        ];
        let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(memory, Captured::new());
        assert_eq!(helper_f4c584(&mut context, callback(), GuestAddress::new(0), GuestAddress::new(0x1000), GuestAddress::new(0)), Ok(0x86));
        assert_eq!(context.read_u32(GuestAddress::new(0x016d4280)), Ok(0x63636363));
        assert_eq!(context.read_u8(GuestAddress::new(0x016d4753)), Ok(0x63));
        assert_eq!(context.effects.cache, Some((GuestAddress::new(0x016d4280), ByteCount(0x4c0))));
        assert!(context.effects.envelope.is_none());
    }
}
