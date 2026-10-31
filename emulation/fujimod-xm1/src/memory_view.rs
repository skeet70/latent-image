use fujimod_emulator::board::factory::MemoryMap;

#[derive(Clone, Copy)]
pub(crate) struct MemoryCore(pub(crate) usize);

#[derive(Clone, Copy)]
pub(crate) struct MemoryAddress(pub(crate) u64);

#[derive(Clone, Copy)]
pub(crate) struct TcmRegister(pub(crate) u32);

pub(crate) struct StaticSpan<'a> {
    pub(crate) physical: MemoryAddress,
    pub(crate) single: Option<&'a MemoryMap>,
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

// Unique readable coverage with one affine physical translation.
pub(crate) fn static_span(maps: &[MemoryMap], core: MemoryCore, extent: std::ops::Range<MemoryAddress>) -> Result<StaticSpan<'_>, String> {
    let (start, end) = (extent.start.0, extent.end.0);
    require(start < end && end <= 1 << 32, "static extent is empty or exceeds 32 bits")?;
    let mut cursor = start;
    let mut physical: Option<u64> = None;
    let mut single = None;
    while cursor < end {
        let mut selected = None;
        let mut next = end;
        for map in maps.iter().filter(|map| map.core == core.0) {
            let map_end = map.address.checked_add(map.size as u64).ok_or("static mapping address overflows")?;
            let physical_end = map.physical.checked_add(map.size as u64).ok_or("static mapping physical extent overflows")?;
            require(map.size != 0 && map_end <= 1 << 32 && physical_end <= 1 << 32,
                "static mapping is empty or exceeds 32 bits")?;
            if map.address >= end || map_end <= start { continue; }
            require(map.permissions & 1 != 0, "static extent includes an unreadable mapping")?;
            if map.address > cursor {
                next = next.min(map.address);
            } else if cursor < map_end {
                require(selected.is_none(), "static extent has overlapping mappings")?;
                selected = Some(map);
                next = next.min(map_end);
            }
        }
        let map = selected.ok_or("static extent has no mapping")?;
        let translated = map.physical.checked_add(cursor - map.address).ok_or("static translation overflows")?;
        if let Some(base) = physical {
            require(base.checked_add(cursor - start) == Some(translated), "static extent has noncontiguous physical translation")?;
        } else {
            physical = Some(translated);
            single = Some(map);
        }
        if next < end { single = None; }
        cursor = next;
    }
    Ok(StaticSpan { physical: MemoryAddress(physical.ok_or("static extent has no translation")?), single })
}

// Cortex-R TCM base [31:12], size code [6:2], enable [0].
pub(crate) fn tcm_window(bank: TcmRegister) -> Result<Option<(u64, u64)>, String> {
    let code = (bank.0 >> 2) & 0x1f;
    if bank.0 & 1 == 0 || code == 0 { return Ok(None); }
    require((3..=14).contains(&code), "TCM region size encoding is reserved")?;
    let base = u64::from(bank.0 & 0xffff_f000);
    let size = 512u64 << code;
    require(base % size == 0 && base + size <= 1 << 32, "TCM region is misaligned or extends past 4 GiB")?;
    Ok(Some((base, base + size)))
}
