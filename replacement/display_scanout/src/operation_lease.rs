use crate::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseError { Uninitialized, Held }
impl LeaseError {
    pub const fn stock(self) -> i32 { match self { Self::Uninitialized => -10, Self::Held => -12 } }
}

pub trait OperationLease {
    fn selected(&self) -> Engine;
    fn initialized(&self, engine: Engine) -> bool;
    fn count(&self, engine: Engine) -> u8;
    fn set_count(&mut self, engine: Engine, value: u8);
    fn reset(&mut self, engine: Engine);
}

/// Pinned 0x6c9214.
pub fn acquire(runtime: &mut impl OperationLease, engine: Engine) -> Result<(), LeaseError> {
    if !runtime.initialized(engine) { return Err(LeaseError::Uninitialized); }
    let count = runtime.count(engine);
    if count != 0 {
        runtime.set_count(engine, count.wrapping_add(1));
        return Err(LeaseError::Held);
    }
    runtime.set_count(engine, 1);
    runtime.reset(engine);
    Ok(())
}

/// Pinned 0x6c8ee8.
pub fn release(runtime: &mut impl OperationLease, engine: Engine) -> Result<(), LeaseError> {
    if !runtime.initialized(engine) { return Err(LeaseError::Uninitialized); }
    let count = runtime.count(engine).wrapping_sub(1);
    runtime.set_count(engine, count);
    if count == 0 { Ok(()) } else { Err(LeaseError::Held) }
}

/// Pinned 0x6d16c8.
pub fn operation_lease(runtime: &mut impl OperationLease, value: u32) -> i32 {
    let engine = runtime.selected();
    let result = if value == 1 { release(runtime, engine) } else { acquire(runtime, engine) };
    result.map_or_else(LeaseError::stock, |()| 0)
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod native {
    use super::*;
    use crate::layout;
    use core::ptr::{read_volatile, write_volatile};

    struct Live;
    impl OperationLease for Live {
        fn selected(&self) -> Engine {
            if unsafe { read_volatile(0x955b20 as *const u32) == 0 && read_volatile(0x955b08 as *const u32) == 1 } { Engine::One } else { Engine::Zero }
        }
        fn initialized(&self, engine: Engine) -> bool { unsafe { read_volatile((layout::runtime_base(engine) + 0x54) as *const u8) != 0 } }
        fn count(&self, engine: Engine) -> u8 { unsafe { read_volatile((layout::runtime_base(engine) + 0x55) as *const u8) } }
        fn set_count(&mut self, engine: Engine, value: u8) { unsafe { write_volatile((layout::runtime_base(engine) + 0x55) as *mut u8, value); } }
        fn reset(&mut self, engine: Engine) { unsafe { let _ = crate::abi::xm1_scanout_reset(engine.index() as u32); } }
    }
    fn status(result: Result<(), LeaseError>) -> i32 { result.map_or_else(LeaseError::stock, |()| 0) }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_operation_acquire(engine: u32) -> i32 {
        let Some(engine) = Engine::parse(engine) else { return crate::Error::InvalidArgument.stock(); };
        status(acquire(&mut Live, engine))
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_operation_release(engine: u32) -> i32 {
        let Some(engine) = Engine::parse(engine) else { return crate::Error::InvalidArgument.stock(); };
        status(release(&mut Live, engine))
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_operation_lease(value: u32) -> i32 { operation_lease(&mut Live, value) }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{xm1_scanout_operation_acquire, xm1_scanout_operation_lease, xm1_scanout_operation_release};
