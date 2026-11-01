use super::*;

// Block metrics include native headers, alignment padding and internal slack.
// Pool-managed bytes include in-pool metadata and sentinels; allocated bytes exclude permanent metadata.
// External metadata is the complete reserved STATE storage plus the exported status word.
impl State {
    pub(super) unsafe fn footprint(&self, pointer: NonNull<u8>, layout: NativeLayout) -> Result<usize, Status> {
        match &self.engine {
            // rlsf =0.2.3 unstable diagnostics are benchmark-only.
            #[cfg(feature = "rlsf")]
            Engine::Rlsf { core, admitted } => {
                let address = pointer.as_ptr() as usize;
                for block in core.iter_blocks(admitted.slice()) {
                    let base = block.as_ptr().as_ptr() as *mut u8 as usize;
                    if block.is_occupied() && address >= base && address + layout.0.size() <= base + block.size() {
                        return Ok(block.size());
                    }
                }
                Err(Status::Invalid)
            }
            #[cfg(feature = "talc")]
            Engine::Talc { .. } => {
                let bytes = layout.talc_block_bytes();
                let base = pointer.as_ptr() as usize;
                if !self.pool.contains(base, bytes) { return Err(Status::Invalid); }
                // talc 5.1.1 tag.rs:43-47. No remainder absorption occurs in try_allocate.
                if read_word(base + bytes - size_of::<usize>()) & 1 == 0 { return Err(Status::Invalid); }
                Ok(bytes)
            }
            #[cfg(feature = "buddy")]
            Engine::Buddy { geometry, .. } => {
                geometry.native_pointer(pointer, layout)?;
                layout.buddy_block_bytes()
            }
        }
    }

    pub(super) unsafe fn stats(&self) -> Result<Stats, Status> {
        let (allocated, free, largest, managed) = match &self.engine {
            #[cfg(feature = "rlsf")]
            Engine::Rlsf { core, admitted } => {
                let mut allocated = 0;
                let mut free = 0;
                let mut largest = 0;
                for block in core.iter_blocks(admitted.slice()) {
                    if block.is_occupied() {
                        allocated += block.size();
                    } else {
                        free += block.size();
                        largest = largest.max(block.size());
                    }
                }
                (allocated, free, largest, admitted.bytes)
            }
            #[cfg(feature = "talc")]
            Engine::Talc { metadata_bytes, .. } => {
                let (free, largest) = self.talc_free_blocks()?;
                let allocated = self.pool.bytes.checked_sub(*metadata_bytes)
                    .and_then(|capacity| capacity.checked_sub(free)).ok_or(Status::Invalid)?;
                (allocated, free, largest, self.pool.bytes)
            }
            #[cfg(feature = "buddy")]
            Engine::Buddy { geometry, .. } => {
                let (free, largest) = self.buddy_free_blocks(geometry)?;
                let allocated = geometry.capacity.checked_sub(free).ok_or(Status::Invalid)?;
                (allocated, free, largest, self.pool.bytes)
            }
        };
        Ok(Stats {
            allocated_block_bytes: allocated as u32,
            free_block_bytes: free as u32,
            largest_free_block_bytes: largest as u32,
            external_metadata_bytes: (size_of::<Option<State>>() + size_of::<u32>()) as u32,
            pool_managed_bytes: managed as u32,
        })
    }

    #[cfg(feature = "talc")]
    unsafe fn talc_free_blocks(&self) -> Result<(usize, usize), Status> {
        // talc 5.1.1 base/mod.rs:633-653, chunk.rs:36-66, node.rs:12-16.
        let mut free = 0usize;
        let mut largest = 0usize;
        let mut remaining = self.pool.bytes / talc::base::CHUNK_UNIT;
        for bin in 0..DefaultBinning::BIN_COUNT as usize {
            let mut node = read_word(self.pool.start() + bin * size_of::<usize>());
            while node != 0 {
                if remaining == 0 || node % talc::base::CHUNK_UNIT != 0
                    || !self.pool.contains(node, talc::base::CHUNK_UNIT)
                {
                    return Err(Status::Invalid);
                }
                remaining -= 1;
                let bytes = read_word(node + 3 * size_of::<usize>());
                if bytes == 0 || bytes % talc::base::CHUNK_UNIT != 0 || !self.pool.contains(node, bytes) {
                    return Err(Status::Invalid);
                }
                free = free.checked_add(bytes).ok_or(Status::Invalid)?;
                largest = largest.max(bytes);
                node = read_word(node);
            }
        }
        Ok((free, largest))
    }

    #[cfg(feature = "buddy")]
    unsafe fn buddy_free_blocks(&self, geometry: &BuddyGeometry) -> Result<(usize, usize), Status> {
        // buddy-alloc 0.6.0 buddy_alloc.rs:75-125,223-239,330-385.
        let mut free = 0usize;
        let mut largest = 0usize;
        let mut remaining = geometry.capacity / LEAF_BYTES;
        for order in 0..geometry.orders {
            let head = geometry.head_base + order * 2 * size_of::<usize>();
            let bytes = LEAF_BYTES << order;
            // Either pointer field traverses the circular doubly linked list.
            let mut node = read_word(head);
            while node != head {
                if remaining == 0 || node < geometry.managed_base
                    || (node - geometry.managed_base) % bytes != 0
                    || !self.pool.contains(node, bytes)
                {
                    return Err(Status::Invalid);
                }
                remaining -= 1;
                free = free.checked_add(bytes).ok_or(Status::Invalid)?;
                largest = largest.max(bytes);
                node = read_word(node);
            }
        }
        Ok((free, largest))
    }
}

#[cfg(any(feature = "talc", feature = "buddy"))]
unsafe fn read_word(address: usize) -> usize {
    (address as *const usize).read()
}
