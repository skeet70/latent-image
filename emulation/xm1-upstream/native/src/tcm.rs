//! Architected, per-core TCM registers and persistent physical RAM.
//! ARM DDI0363E 4.2.21/22, 3.1.5: size is read-only; changing the base
//! changes the view of the same RAM. Opc2=0 is BTCM, opc2=1 is ATCM.
use crate::engine::{Backing, Inner};
use crate::ffi;

pub type Parameters = Vec<(usize, u32)>; // BTCM, ATCM: bytes, reset base/enable
pub type Snapshot = Vec<(usize, u32, u32, Vec<u8>)>;

#[derive(Clone, Copy)]
pub struct Mapping {
    pub address: u64,
    pub size: usize,
    pub permissions: u32,
    pub pointer: usize,
}

pub struct Bank {
    pub reset: u32,
    pub register: u32,
    pub memory: Backing,
    pub shadows: Vec<Mapping>,
    pub store_hook: Option<usize>,
}

pub struct Tcm {
    pub banks: [[Bank; 2]; 2],
    pub pending: Option<(usize, usize, u32)>,
}

pub fn normalize(size: usize, word: u32) -> Result<u32, String> {
    if size == 0 {
        if word & 1 != 0 { return Err("Cannot enable an absent TCM interface".into()); }
        return Ok(word & 0xfffff000);
    }
    if !(0x1000..=0x800000).contains(&size) || !size.is_power_of_two() {
        return Err("TCM physical size must be zero or a power of two from4KiB to8MiB".into());
    }
    Ok((word & !(size as u32 - 1)) | ((size.trailing_zeros() - 9) << 2) | (word & 1))
}

impl Tcm {
    pub fn new(parameters: &Parameters) -> Result<Self, String> {
        if parameters.len() != 2 { return Err("Require BTCM and ATCM SoC parameters".into()); }
        let regs = [normalize(parameters[0].0, parameters[0].1)?,
                    normalize(parameters[1].0, parameters[1].1)?];
        validate_disjoint(parameters[0].0, regs[0], parameters[1].0, regs[1])?;
        if parameters.iter().map(|p| p.0).sum::<usize>() > 0x1000000 {
            return Err("TCM physical allocation exceeds its SoC bound".into());
        }
        Ok(Self { banks: std::array::from_fn(|_| std::array::from_fn(|i| Bank {
            reset: regs[i], register: regs[i], memory: Backing::zeros(parameters[i].0), shadows: Vec::new(), store_hook: None,
        })), pending: None })
    }
    pub fn extent(&self, core: usize, bank: usize) -> Option<(u64, usize)> {
        let b = &self.banks[core][bank];
        (b.register & 1 != 0 && b.memory.len() != 0)
            .then_some(((b.register & 0xfffff000) as u64, b.memory.len()))
    }
    pub fn locate(&self, core: usize, address: u64, size: usize) -> Option<(usize, usize)> {
        (0..2).find_map(|i| self.extent(core, i).and_then(|(base, bytes)| {
            (address >= base && address.checked_add(size as u64)? <= base + bytes as u64)
                .then_some((i, (address-base) as usize))
        }))
    }
    pub fn snapshot(&self, core: usize) -> Snapshot {
        self.banks[core].iter().map(|b| (b.memory.len(), b.reset, b.register,
            unsafe { std::slice::from_raw_parts(b.memory.as_ptr(), b.memory.len()) }.to_vec())).collect()
    }
    pub fn default_layout(&self) -> bool {
        self.banks.iter().all(|banks|
            banks[0].memory.len() == 0 && banks[0].reset == 0 && banks[0].register == 0 &&
            banks[1].memory.len() == 0x2000 && banks[1].reset == 0xffff8011 &&
            banks[1].register == 0xffff8011)
    }
}

pub fn locate(inner: &Inner, core: usize, address: u64, size: usize) -> Option<(usize, usize)> {
    unsafe { (&*inner.tcm.get()).locate(core, address, size) }
}

pub fn regions(inner: &Inner, core: usize) -> Vec<(u64, usize)> {
    unsafe { let t = &*inner.tcm.get(); (0..2).filter_map(|i| t.extent(core,i)).collect() }
}

pub fn register(inner: &Inner, core: usize, bank: usize) -> u32 {
    unsafe { (&*inner.tcm.get()).banks[core][bank].register }
}

pub fn initial_map(inner: &Inner, core: usize, bank: usize) -> Result<(), String> {
    let Some((base, size)) = (unsafe { (&*inner.tcm.get()).extent(core, bank) }) else { return Ok(()); };
    // Existing high-stack constructor memory was the legacy TCM backing,
    // not a second memory underneath the newly explicit TCM.
    map(inner, core, bank, base, size, true)
}

fn map(inner: &Inner, core: usize, bank: usize, base: u64, size: usize, initial: bool) -> Result<(), String> {
    let end = base + size as u64;
    if end > 1 << 32 { return Err("TCM extent exceeds ARM32 address space".into()); }
    for (other_base, other_size) in regions(inner, core) {
        if other_base != base && other_base < end && base < other_base + other_size as u64 {
            return Err("ATCM and BTCM must not overlap".into());
        }
    }
    let spans = inner.memory_regions(core)?;
    let mut shadows = Vec::new();
    for (lo, hi, permissions) in spans {
        let begin = lo.max(base); let finish = (hi + 1).min(end);
        if begin >= finish { continue; }
        let bytes = (finish-begin) as usize;
        let pointer = inner.owned_pointer(core, begin, bytes, permissions)
            .ok_or_else(|| format!("TCM overlay of unowned memory at{begin:#x} is unmodeled"))?;
        // Only the legacy X70 ATCM window replaces anonymous constructor RAM.
        // An explicit SoC reset over DRAM/ROM must retain that real underlay.
        let legacy_atcm = initial && bank == 1 && base == 0xffff8000 && size == 0x2000;
        if !legacy_atcm { shadows.push(Mapping { address: begin, size: bytes, permissions, pointer }); }
    }
    // Validation above is complete before changing any mapping.
    for (lo, hi, _) in inner.memory_regions(core)? {
        let begin = lo.max(base); let finish = (hi+1).min(end);
        if begin < finish {
            inner.check(unsafe { (inner.api.uc_mem_unmap)(inner.core(core)?, begin, (finish-begin) as usize) })?;
        }
    }
    let pointer = unsafe { (&*inner.tcm.get()).banks[core][bank].memory.as_ptr() as usize };
    inner.map_owned(core, base, size, 7, pointer)?;
    let covered = (base<inner.ram.len() as u64 && end<=inner.ram.len() as u64)
        || (base>=0x40000000 && end<=0x40000000+inner.ram.len() as u64)
        || (base>=0xffff0000 && end<=1<<32);
    if !covered {
        let data=(&*inner.hook_data[core] as *const crate::engine::HookData).cast_mut().cast();
        let hook=inner.add_hook(core,1<<11,crate::hooks::store as *const () as *mut std::ffi::c_void,data,base,end-1)?;
        unsafe { (&mut *inner.tcm.get()).banks[core][bank].store_hook=Some(hook); }
    }
    unsafe { (&mut *inner.tcm.get()).banks[core][bank].shadows = shadows; }
    Ok(())
}

pub fn set(inner: &Inner, core: usize, bank: usize, value: u32) -> Result<(), String> {
    let (old, size) = unsafe { let b = &(&*inner.tcm.get()).banks[core][bank]; (b.register,b.memory.len()) };
    let value = normalize(size,value)?;
    if value == old { return Ok(()); }
    // Preflight the target against the other TCM and the machine's owned maps.
    if value & 1 != 0 {
        let base = (value & 0xfffff000) as u64; let end = base + size as u64;
        for i in 0..2 {
            if i != bank {
                if let Some((p,n)) = unsafe { (&*inner.tcm.get()).extent(core,i) } {
                    if p < end && base < p+n as u64 { return Err("ATCM and BTCM must not overlap".into()); }
                }
            }
        }
        for (lo,hi,permissions) in inner.memory_regions(core)? {
            let begin=lo.max(base); let finish=(hi+1).min(end);
            if begin<finish && locate(inner,core,begin,(finish-begin) as usize).is_none()
                && inner.owned_pointer(core,begin,(finish-begin) as usize,permissions).is_none() {
                return Err("TCM overlay has unowned backing".into());
            }
        }
    }
    if let Some((base,n)) = unsafe { (&*inner.tcm.get()).extent(core,bank) } {
        if let Some(hook)=unsafe { (&mut *inner.tcm.get()).banks[core][bank].store_hook.take() } {
            inner.check(unsafe { (inner.api.uc_hook_del)(inner.core(core)?,hook) })?;
        }
        inner.check(unsafe { (inner.api.uc_mem_unmap)(inner.core(core)?,base,n) })?;
        let shadows = unsafe { std::mem::take(&mut (&mut *inner.tcm.get()).banks[core][bank].shadows) };
        for m in shadows { inner.map_owned(core,m.address,m.size,m.permissions,m.pointer)?; }
    }
    unsafe { (&mut *inner.tcm.get()).banks[core][bank].register = value; }
    if value & 1 != 0 { map(inner,core,bank,(value&0xfffff000) as u64,size,false)?; }
    unsafe {
        let state=&mut *inner.state.get();
        state.reservations[core]=None;
        state.tcm_windows[core][bank]=(&*inner.tcm.get()).extent(core,bank);
        state.clear_derived();
        state.touch();
    }
    inner.check(unsafe { (inner.api.uc_ctl)(inner.core(core)?,ffi::ctl(10,0,1)) })?;
    Ok(())
}

pub fn request(inner: &Inner, core: usize, bank: usize, value: u32) -> Result<bool,String> {
    let size=unsafe { (&*inner.tcm.get()).banks[core][bank].memory.len() };
    let value=normalize(size,value)?;
    if value==register(inner,core,bank) { return Ok(false); }
    unsafe { (&mut *inner.tcm.get()).pending=Some((core,bank,value)); }
    Ok(true)
}

pub fn restore(inner: &Inner, core: usize, snapshot: &Snapshot) -> Result<(),String> {
    if snapshot.len()!=2 { return Err("TCM snapshot requires two banks".into()); }
    for (i,(size,reset,reg,data)) in snapshot.iter().enumerate() {
        let b=unsafe { &(&*inner.tcm.get()).banks[core][i] };
        if *size!=b.memory.len() || *reset!=b.reset || data.len()!=*size || normalize(*size,*reg)?!=*reg {
            return Err("TCM SoC parameters or snapshot extent differ".into());
        }
    }
    validate_disjoint(snapshot[0].0, snapshot[0].2, snapshot[1].0, snapshot[1].2)?;
    let changed=snapshot.iter().enumerate().any(|(i,p)|p.2!=register(inner,core,i));
    // Disable both only when the mapping changes; pure register inspection
    // must leave monitors, caches and the existing constructor map untouched.
    if changed { for i in 0..2 { let r=register(inner,core,i); set(inner,core,i,r&!1)?; } }
    for (i,(_,_,reg,data)) in snapshot.iter().enumerate() {
        let b=unsafe { &(&*inner.tcm.get()).banks[core][i] };
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(),b.memory.as_ptr(),data.len()); }
        set(inner,core,i,*reg)?;
    }
    Ok(())
}

fn validate_disjoint(first_size: usize, first: u32, second_size: usize, second: u32) -> Result<(), String> {
    if first & 1 != 0 && second & 1 != 0 {
        let a = (first & 0xfffff000) as u64;
        let b = (second & 0xfffff000) as u64;
        if a < b + second_size as u64 && b < a + first_size as u64 {
            return Err("ATCM and BTCM must not overlap".into());
        }
    }
    Ok(())
}

pub fn base_layout(inner: &Inner, core: usize) -> Result<Vec<(u64,u64,u32)>,String> {
    let mut spans=inner.memory_regions(core)?;
    for bank in 0..2 {
        if let Some((base,n))=unsafe { (&*inner.tcm.get()).extent(core,bank) } {
            let end=base+n as u64;
            spans=spans.into_iter().flat_map(|(lo,hi,p)| {
                let mut out=Vec::new();
                if hi<base || lo>=end { out.push((lo,hi,p)); }
                else { if lo<base { out.push((lo,base-1,p)); } if hi>=end { out.push((end,hi,p)); } }
                out
            }).collect();
            for m in unsafe { &(&*inner.tcm.get()).banks[core][bank].shadows } {
                spans.push((m.address,m.address+m.size as u64-1,m.permissions));
            }
        }
    }
    spans.sort_unstable();
    let mut merged:Vec<(u64,u64,u32)>=Vec::new();
    for (lo,hi,p) in spans {
        if let Some(last)=merged.last_mut() {
            if last.1+1==lo && last.2==p { last.1=hi; continue; }
        }
        merged.push((lo,hi,p));
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly_size_alignment_and_reserved_bits() {
        assert_eq!(normalize(0x2000,0xffff8411).unwrap(),0xffff8011);
        assert_eq!(normalize(0x2000,0x7000007d).unwrap(),0x70000011);
        assert!(normalize(0,1).is_err());
        assert!(normalize(0x1800,0).is_err());
    }
    #[test]
    fn initial_and_restored_banks_must_be_disjoint() {
        assert!(Tcm::new(&vec![(0x2000,0x70000001),(0x2000,0x70000001)]).is_err());
        assert!(Tcm::new(&vec![(0x2000,0x70000001),(0x2000,0x70002001)]).is_ok());
        assert!(validate_disjoint(0x2000,0x70000011,0x2000,0x70000011).is_err());
        assert!(validate_disjoint(0x2000,0x70000010,0x2000,0x70000011).is_ok());
    }
}
