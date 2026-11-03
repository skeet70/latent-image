#![no_std]

use core::mem::MaybeUninit;

use aarch32_rt::entry;
use embassy_executor::{Executor, Spawner};

static mut EXECUTOR: MaybeUninit<Executor> = MaybeUninit::uninit();

#[embassy_executor::task]
async fn idle() {
    core::future::pending::<()>().await
}

#[entry]
fn main() -> ! {
    // SAFETY: `main` runs once on one core and never returns, so this is the only reference.
    let executor = unsafe { (*(&raw mut EXECUTOR)).write(Executor::new()) };
    executor.run(|spawner: Spawner| spawner.spawn(idle().unwrap()))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
