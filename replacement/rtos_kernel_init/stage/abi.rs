//! Fixed-bank bodies of the kernel startup group, exported as
//! `xm1_rtos_kernel_<entry>_<domain>`. `probe` adds a `b` veneer at each stock entry and a
//! panic handler for a standalone link.

#![no_std]

#[allow(dead_code)]
#[path = "../src/lib.rs"]
mod startup;

#[cfg(feature = "domain-a")]
#[allow(unused_attributes, dead_code)]
#[path = "../../primary_initializer/src.rs"]
mod primary_publisher;

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use startup::layout::{Domain, GuestAddress};
use startup::{CodeAddress, Machine, Phase, PhaseRoutine, Startup};

#[cfg(feature = "domain-a")]
const DOMAIN: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const DOMAIN: Domain = Domain::B;

static STARTUP: Startup = Startup::of(DOMAIN);

#[cfg(feature = "domain-a")]
macro_rules! bank_name {
    ($entry:literal) => {
        concat!("xm1_rtos_", $entry, "_a")
    };
}
#[cfg(feature = "domain-b")]
macro_rules! bank_name {
    ($entry:literal) => {
        concat!("xm1_rtos_", $entry, "_b")
    };
}

struct Cpu;

impl Machine for Cpu {
    #[inline(always)]
    fn load(&self, at: GuestAddress) -> u32 {
        unsafe { read_volatile(at.0 as *const u32) }
    }

    #[inline(always)]
    fn store(&self, at: GuestAddress, value: u32) {
        unsafe { write_volatile(at.0 as *mut u32, value) }
    }

    #[inline(always)]
    fn load_byte(&self, at: GuestAddress) -> u8 {
        unsafe { read_volatile(at.0 as *const u8) }
    }

    #[inline(always)]
    fn store_byte(&self, at: GuestAddress, value: u8) {
        unsafe { write_volatile(at.0 as *mut u8, value) }
    }

    #[inline(always)]
    fn call(&self, routine: CodeAddress, arguments: [u32; 3]) -> u32 {
        let routine = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32, u32) -> u32>(routine.0 as usize) };
        routine(arguments[0], arguments[1], arguments[2])
    }

    #[inline(always)]
    fn call_with_record(&self, routine: CodeAddress, first: u32, record: &[u32; 5]) -> u32 {
        let routine = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, *const u32) -> u32>(routine.0 as usize) };
        routine(first, record.as_ptr())
    }

    #[inline(always)]
    fn phase(&self, routine: PhaseRoutine, phase: Phase) -> u32 {
        let routine = unsafe { core::mem::transmute::<usize, extern "C" fn(u32) -> u32>(routine.0 as usize) };
        routine(phase as u32)
    }

    #[inline(always)]
    fn publish_interfaces(&self) {
        #[cfg(feature = "domain-a")]
        unsafe {
            primary_publisher::xm1_primary_initializer_publish()
        };
        #[cfg(feature = "domain-b")]
        startup::interfaces_b::publish(self);
    }

    #[inline(always)]
    fn barrier(&self) {
        unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn pause(&self) {
        unsafe { asm!("nop", options(nomem, nostack, preserves_flags)) };
    }
}

#[export_name = bank_name!("kernel_publish_interfaces")]
#[inline(never)]
#[link_section = ".text.kernel_body_publish_interfaces"]
pub extern "C" fn publish_interfaces() {
    Cpu.publish_interfaces();
}

#[export_name = bank_name!("kernel_start")]
#[inline(never)]
#[link_section = ".text.kernel_body_start"]
pub extern "C" fn kernel_start(phase: u32) -> ! {
    startup::kernel_start(&Cpu, &STARTUP, PhaseRoutine(phase));
    loop {
        core::hint::spin_loop();
    }
}

#[export_name = bank_name!("kernel_start_tick_timer")]
#[inline(never)]
#[link_section = ".text.kernel_body_start_tick_timer"]
pub extern "C" fn start_tick_timer() {
    startup::start_tick_timer(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_configure_core")]
#[inline(never)]
#[link_section = ".text.kernel_body_configure_core"]
pub extern "C" fn configure_core() {
    startup::configure_core(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_owner_contexts")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_owner_contexts"]
pub extern "C" fn initialize_owner_contexts() {
    startup::initialize_owner_contexts(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_tasks")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_tasks"]
pub extern "C" fn initialize_tasks() {
    startup::initialize_tasks(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_autostart_tasks")]
#[inline(never)]
#[link_section = ".text.kernel_body_autostart_tasks"]
pub extern "C" fn autostart_tasks() {
    startup::autostart_tasks(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_objects")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_objects"]
pub extern "C" fn initialize_objects() {
    startup::initialize_objects(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_semaphores")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_semaphores"]
pub extern "C" fn initialize_semaphores() {
    startup::initialize_semaphores(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_event_flags")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_event_flags"]
pub extern "C" fn initialize_event_flags() {
    startup::initialize_event_flags(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_data_queues")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_data_queues"]
pub extern "C" fn initialize_data_queues() {
    startup::initialize_data_queues(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_mailboxes")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_mailboxes"]
pub extern "C" fn initialize_mailboxes() {
    startup::initialize_mailboxes(&Cpu, &STARTUP);
}

#[export_name = bank_name!("kernel_initialize_fixed_pools")]
#[inline(never)]
#[link_section = ".text.kernel_body_initialize_fixed_pools"]
pub extern "C" fn initialize_fixed_pools() {
    startup::initialize_fixed_pools(&Cpu, &STARTUP);
}

#[cfg(feature = "probe")]
macro_rules! probe_veneer {
    ($entry:literal, $domain:literal) => {
        concat!(
            ".pushsection .text.kernel_veneer_", $entry, ", \"ax\", %progbits\n",
            ".arm\n",
            ".global xm1_rtos_", $entry, "\n",
            ".type xm1_rtos_", $entry, ", %function\n",
            "xm1_rtos_", $entry, ":\n",
            "b xm1_rtos_", $entry, "_", $domain, "\n",
            ".size xm1_rtos_", $entry, ", . - xm1_rtos_", $entry, "\n",
            ".popsection\n",
        )
    };
}

#[cfg(feature = "probe")]
macro_rules! probe_veneers {
    ($domain:literal) => {
        concat!(
            probe_veneer!("kernel_publish_interfaces", $domain),
            probe_veneer!("kernel_start", $domain),
            probe_veneer!("kernel_start_tick_timer", $domain),
            probe_veneer!("kernel_configure_core", $domain),
            probe_veneer!("kernel_initialize_owner_contexts", $domain),
            probe_veneer!("kernel_initialize_tasks", $domain),
            probe_veneer!("kernel_autostart_tasks", $domain),
            probe_veneer!("kernel_initialize_semaphores", $domain),
            probe_veneer!("kernel_initialize_event_flags", $domain),
            probe_veneer!("kernel_initialize_data_queues", $domain),
            probe_veneer!("kernel_initialize_mailboxes", $domain),
            probe_veneer!("kernel_initialize_fixed_pools", $domain),
            probe_veneer!("kernel_initialize_objects", $domain),
        )
    };
}

#[cfg(all(feature = "probe", feature = "domain-a"))]
core::arch::global_asm!(probe_veneers!("a"));
#[cfg(all(feature = "probe", feature = "domain-b"))]
core::arch::global_asm!(probe_veneers!("b"));

#[cfg(feature = "probe")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
