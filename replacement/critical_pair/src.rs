//! Task103 critical-section boundaries over retained loader services.
#![no_std]

#[allow(unused_attributes)]
#[path = "../interface_initializer/src.rs"]
mod interface_initializer;

use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy)]
struct CoreIndex(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct PriorIrqMask(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct InterfaceOffset(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct GuestAddress(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct RetainedCode(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Tag(u32);

#[derive(Clone, Copy)]
enum TagKind {
    Direct,
    Counted,
}
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Counter(u32);
#[repr(transparent)]
struct Cpsr(u32);

const ROOTS: GuestAddress = GuestAddress(0x0096_bd20);
const COUNTER: GuestAddress = GuestAddress(0x40a0_f824);
const COUNTER_BASE: GuestAddress = GuestAddress(0x40a0_f3d0);
const IDENTITY: InterfaceOffset = InterfaceOffset(0x1618);
const RETRY: InterfaceOffset = InterfaceOffset(0x161c);
const RELEASE: InterfaceOffset = InterfaceOffset(0x1620);
const OBSERVE: InterfaceOffset = InterfaceOffset(0x1624);
const RESTORE_IRQ: InterfaceOffset = InterfaceOffset(0x168);

#[repr(C)]
pub(crate) struct Scratch {
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
    r12: u32,
    lr: u32,
    cpsr: Cpsr,
}

extern "C" {
    fn xm1_critical_retained_call(scratch: *mut Scratch, target: RetainedCode);
}

impl GuestAddress {
    #[inline(always)]
    unsafe fn load(self) -> u32 {
        read_volatile(self.0 as *const u32)
    }

    #[inline(always)]
    unsafe fn store(self, value: u32) {
        write_volatile(self.0 as *mut u32, value);
    }
}

impl CoreIndex {
    #[inline(always)]
    fn tag(self, kind: TagKind) -> Tag {
        let bits = match kind {
            TagKind::Direct => 0x10,
            TagKind::Counted => 0x20,
        };
        Tag(self.0.wrapping_add(1) | bits)
    }

    #[inline(always)]
    unsafe fn root(self) -> GuestAddress {
        GuestAddress(GuestAddress(ROOTS.0.wrapping_add(self.0.wrapping_shl(2))).load())
    }
}

impl Scratch {
    #[inline(always)]
    fn compare(&mut self, left: u32, right: u32) {
        let result = left.wrapping_sub(right);
        let negative = result & 0x8000_0000;
        let zero = u32::from(result == 0) << 30;
        let carry = u32::from(left >= right) << 29;
        let overflow = ((left ^ right) & (left ^ result) & 0x8000_0000) >> 3;
        self.cpsr.0 = (self.cpsr.0 & !0xf000_0000) | negative | zero | carry | overflow;
    }

    #[inline(always)]
    unsafe fn call(&mut self) {
        xm1_critical_retained_call(self, RetainedCode(self.r3));
    }

    #[inline(always)]
    unsafe fn observe(&mut self, core: CoreIndex) -> Tag {
        self.r2 = core.root().0;
        self.r3 = GuestAddress(self.r2.wrapping_add(OBSERVE.0)).load();
        self.r0 = 0;
        self.call();
        Tag(self.r0)
    }

    #[inline(always)]
    unsafe fn increment(&mut self) {
        self.r3 = COUNTER_BASE.0;
        self.r2 = Counter(COUNTER.load()).0.wrapping_add(1);
        COUNTER.store(self.r2);
    }

    #[inline(always)]
    unsafe fn restore(&mut self, core: CoreIndex, prior: PriorIrqMask, continuation: GuestAddress) {
        self.r3 = core.root().0;
        self.r3 = GuestAddress(self.r3.wrapping_add(RESTORE_IRQ.0)).load();
        self.r0 = prior.0;
        self.call();
        self.lr = continuation.0;
    }
}

// The original prologue has saved caller r10; live r10 carries the prior IRQ mask.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_critical_enter_continue(
    scratch: &mut Scratch,
    core: CoreIndex,
    prior: PriorIrqMask,
) {
    let first = scratch.observe(core);
    scratch.r3 = core.tag(TagKind::Direct).0;
    scratch.compare(first.0, scratch.r3);
    if first.0 != scratch.r3 {
        let second = scratch.observe(core);
        let tag = core.tag(TagKind::Counted);
        scratch.compare(second.0, tag.0);
        if second.0 != tag.0 {
            loop {
                scratch.r3 = core.root().0;
                scratch.r3 = GuestAddress(scratch.r3.wrapping_add(RETRY.0)).load();
                scratch.r0 = 0;
                scratch.r1 = tag.0;
                scratch.call();
                scratch.compare(scratch.r0, 0);
                if scratch.r0 == 0 {
                    break;
                }
            }
        }
        scratch.increment();
    }
    scratch.r0 = prior.0;
}

#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_critical_exit(scratch: &mut Scratch) {
    let prior = PriorIrqMask(scratch.r0);
    scratch.r2 = CoreIndex(0).root().0;
    scratch.r3 = GuestAddress(scratch.r2.wrapping_add(IDENTITY.0)).load();
    scratch.call();
    let core = CoreIndex(scratch.r0);
    let observed = scratch.observe(core);
    scratch.r3 = core.tag(TagKind::Direct).0;
    scratch.compare(observed.0, scratch.r3);
    if observed.0 == scratch.r3 {
        scratch.restore(core, prior, GuestAddress(0x72370c));
        return;
    }
    scratch.r3 = COUNTER.load();
    scratch.compare(scratch.r3, 0);
    if scratch.r3 == 0 {
        scratch.restore(core, prior, GuestAddress(0x72373c));
        return;
    }
    scratch.r2 = Counter(scratch.r3).0.wrapping_sub(1);
    scratch.r3 = COUNTER_BASE.0;
    COUNTER.store(scratch.r2);
    scratch.compare(scratch.r2, 0);
    if scratch.r2 == 0 {
        scratch.r3 = core.root().0;
        scratch.r2 = scratch.r3;
        scratch.r3 = GuestAddress(scratch.r2.wrapping_add(RELEASE.0)).load();
        scratch.r0 = 0;
        scratch.call();
    }
    scratch.restore(core, prior, GuestAddress(0x72378c));
}
