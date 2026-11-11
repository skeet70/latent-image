#[derive(Debug, Eq, PartialEq)]
pub struct NetworkPacketLease {
    slot: usize,
    generation: u64,
    pool: PacketPoolId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PacketIdentity { slot: usize, generation: u64, pool: PacketPoolId }

impl NetworkPacketLease {
    pub(crate) fn identity(&self) -> PacketIdentity {
        PacketIdentity { slot: self.slot, generation: self.generation, pool: self.pool }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketPoolError { Full, TooLarge, StaleLease, OutOfBounds, GenerationExhausted }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PacketPoolId(usize);

pub struct PacketStorage<'a> {
    bytes: &'a mut [u8],
    length: Option<usize>,
    generation: u64,
}

impl<'a> PacketStorage<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self { Self { bytes, length: None, generation: 0 } }
}

pub struct NetworkPacketPool<'a> {
    storage: &'a mut [PacketStorage<'a>],
    id: PacketPoolId,
}

impl<'a> NetworkPacketPool<'a> {
    pub fn new(storage: &'a mut [PacketStorage<'a>]) -> Self {
        let id = PacketPoolId(storage.as_ptr() as usize);
        Self { storage, id }
    }

    pub fn insert(&mut self, bytes: &[u8]) -> Result<NetworkPacketLease, PacketPoolError> {
        let mut free = false;
        for (slot, storage) in self.storage.iter_mut().enumerate() {
            if storage.length.is_some() { continue; }
            free = true;
            if bytes.len() > storage.bytes.len() { continue; }
            let generation = storage.generation.checked_add(1).ok_or(PacketPoolError::GenerationExhausted)?;
            storage.bytes[..bytes.len()].copy_from_slice(bytes);
            storage.length = Some(bytes.len());
            storage.generation = generation;
            return Ok(NetworkPacketLease { slot, generation, pool: self.id });
        }
        Err(if free { PacketPoolError::TooLarge } else { PacketPoolError::Full })
    }

    fn storage(&self, lease: &NetworkPacketLease) -> Result<&PacketStorage<'a>, PacketPoolError> {
        if lease.pool != self.id { return Err(PacketPoolError::StaleLease); }
        let storage = self.storage.get(lease.slot).ok_or(PacketPoolError::StaleLease)?;
        if storage.generation != lease.generation || storage.length.is_none() { return Err(PacketPoolError::StaleLease); }
        Ok(storage)
    }

    pub fn bytes(&self, lease: &NetworkPacketLease) -> Result<&[u8], PacketPoolError> {
        let storage = self.storage(lease)?;
        Ok(&storage.bytes[..storage.length.ok_or(PacketPoolError::StaleLease)?])
    }

    pub fn len(&self, lease: &NetworkPacketLease) -> Result<usize, PacketPoolError> {
        self.storage(lease)?.length.ok_or(PacketPoolError::StaleLease)
    }

    pub fn write(&mut self, lease: &NetworkPacketLease, offset: usize, bytes: &[u8]) -> Result<(), PacketPoolError> {
        let length = self.len(lease)?;
        let end = offset.checked_add(bytes.len()).ok_or(PacketPoolError::OutOfBounds)?;
        if end > length { return Err(PacketPoolError::OutOfBounds); }
        self.storage[lease.slot].bytes[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    pub fn release(&mut self, lease: NetworkPacketLease) -> Result<(), PacketPoolError> {
        self.storage(&lease)?;
        self.storage[lease.slot].length = None;
        Ok(())
    }
}
