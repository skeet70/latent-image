#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheLength(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushSpan { Whole, Lines { start: CacheAddress, end: CacheAddress } }
impl FlushSpan {
    pub fn from_abi(address: CacheAddress, bytes: CacheLength) -> Self {
        let mut length = (address.0 & 31).wrapping_add(bytes.0);
        if length & 31 != 0 { length = (length & !31).wrapping_add(32); }
        let start = if length <= 0x4000 { address.0 & 0xbfff_ffe0 } else { length = 0; 0 };
        if start == 0 || length == 0 { Self::Whole } else { Self::Lines { start: CacheAddress(start), end: CacheAddress(start.wrapping_add(length)) } }
    }
}
pub trait CacheRegisters { fn clean_invalidate_line(&mut self, address: CacheAddress); fn clean_invalidate_all(&mut self); }
pub fn flush(registers: &mut impl CacheRegisters, address: CacheAddress, bytes: CacheLength) {
    match FlushSpan::from_abi(address, bytes) {
        FlushSpan::Whole => registers.clean_invalidate_all(),
        FlushSpan::Lines { start, end } => {
            let mut line = start.0;
            loop {
                registers.clean_invalidate_line(CacheAddress(line));
                line = line.wrapping_add(32);
                if (line as i32) >= (end.0 as i32) { break; }
            }
        }
    }
}
#[cfg(target_arch = "arm")]
pub struct NativeCache;
#[cfg(target_arch = "arm")]
impl CacheRegisters for NativeCache {
    fn clean_invalidate_line(&mut self, address: CacheAddress) { unsafe { core::arch::asm!("mcr p15, 0, {address}, c7, c14, 1", address = in(reg) address.0, options(nostack)); } }
    fn clean_invalidate_all(&mut self) { unsafe extern "C" { fn xm1_power_dcache_clean_invalidate_all(); } unsafe { xm1_power_dcache_clean_invalidate_all(); } }
}
#[cfg(test)]
mod tests {
    use super::{CacheAddress, CacheLength, CacheRegisters, flush};
    #[derive(Default)] struct Cache { lines: [u32; 4], length: usize, whole: bool }
    impl CacheRegisters for Cache {
        fn clean_invalidate_line(&mut self, address: CacheAddress) { self.lines[self.length] = address.0; self.length += 1; }
        fn clean_invalidate_all(&mut self) { self.whole = true; }
    }
    #[test] fn unaligned_cached_alias_publishes_both_intersected_lines() {
        let mut cache = Cache::default(); flush(&mut cache, CacheAddress(0x4169_c03f), CacheLength(2));
        assert_eq!(&cache.lines[..cache.length], &[0x0169_c020, 0x0169_c040]); assert!(!cache.whole);
    }
    #[test] fn large_extent_uses_owned_whole_cache_consumer() {
        let mut cache = Cache::default(); flush(&mut cache, CacheAddress(0x169_c020), CacheLength(0x4001)); assert!(cache.whole); assert_eq!(cache.length,0);
    }
}
