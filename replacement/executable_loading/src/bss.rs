//! Image5 BSS zeroing as X-M1 1.50 `0x6a96c0` does it: a memory-DMA fill requested before the
//! load (`0x6a96f4..0x6a9738`) and awaited after it (`0x6a97f0..0x6a9808`), or a CPU fill and
//! cache publication after the load when the extent is unaligned or the request is refused
//! (`0x6a9810..0x6a9828`).

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bss {
    pub base: u32,
    pub bytes: u32,
}

/// How the extent is being zeroed once the load is under way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fill {
    /// `0x66609c` accepted the request; its completion `0x6a8bec` sets the flag.
    Requested,
    /// The extent is zeroed by the CPU after the load.
    Cpu,
}

pub trait FillPorts {
    /// Stores 0 to the completion flag `0x89f8ac`.
    fn clear_flag(&mut self);
    /// `0x66609c(base, words, 0, 0x6a8bec)`; 0 is an accepted request.
    fn request(&mut self, base: u32, words: u32) -> i32;
    fn flag_set(&mut self) -> bool;
    /// `0x7221c8(base, 0, bytes)`.
    fn cpu_fill(&mut self, base: u32, bytes: u32);
    /// `0x42634(0, 0)`.
    fn publish(&mut self);
}

pub fn begin(bss: Bss, ports: &mut impl FillPorts) -> Fill {
    if bss.base & 3 != 0 || bss.bytes & 3 != 0 {
        return Fill::Cpu;
    }
    ports.clear_flag();
    if ports.request(bss.base, bss.bytes >> 2) == 0 { Fill::Requested } else { Fill::Cpu }
}

pub fn finish(bss: Bss, fill: Fill, ports: &mut impl FillPorts) {
    match fill {
        Fill::Requested => while !ports.flag_set() {},
        Fill::Cpu => {
            ports.cpu_fill(bss.base, bss.bytes);
            ports.publish();
        }
    }
}
