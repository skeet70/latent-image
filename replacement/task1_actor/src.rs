#![no_std]

use core::ptr::{read_volatile, write_volatile};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[repr(u32)]
enum ReceiveOperation { Block = 0x107 }

#[repr(C)]
struct SetupDescriptor {
    record_display: *mut u8,
    system_snapshot: *mut u8,
    delay_state: *mut u8,
    availability: *mut u8,
    buffer: *mut u8,
}

#[repr(C)]
struct Receive {
    operation: ReceiveOperation,
    command: *mut u32,
    argument: *mut u32,
    payload: *mut u32,
}

extern "C" {
    fn setup();
    fn controller_setup();
    fn loop_service();
    fn execution(a: u32, b: u32, c: u32, d: u32, e: u32);
    fn receive(request: *const Receive) -> u32;
    fn low_command();
    fn command_service();
    fn predicate_a() -> u32;
    fn predicate_b() -> u32;
    fn suppression() -> u32;
    fn held_command();
    fn held_clear(value: u32);
    fn pending_first();
    fn pending_second();
    fn image4_pending(value: u32);
    fn pending_predicate() -> u32;
    fn pending_third();
    fn pending_priority();
    fn deferred_first() -> u32;
    fn deferred_second() -> u32;
    fn after_first();
    fn after_second();
    fn after_third();
    fn diagnostic();
    fn pump();
    fn allocate(size: u32) -> *mut u8;
    fn register(descriptor: *const SetupDescriptor) -> u32;
    fn registration_error();
    fn setup_service();
    fn setup_zero(value: u32);
}

unsafe fn word(address: usize) -> u32 { read_volatile(address as *const u32) }
unsafe fn store(address: usize, value: u32) { write_volatile(address as *mut u32, value); }

macro_rules! site {
    ($name:literal) => {
        core::arch::asm!(concat!(".global j11_", $name, "\nj11_", $name, " = .\nnop"), options(nostack, preserves_flags));
    };
}

macro_rules! observed {
    ($name:literal, $value:expr) => {{
        let value = $value;
        site!($name);
        value
    }};
}

#[no_mangle]
#[link_section = ".text.once"]
pub unsafe extern "C" fn xm1_task1_once() {
    store(0x972e84, 1);
    let descriptor = SetupDescriptor {
        record_display: 0x918ab8 as *mut u8,
        system_snapshot: 0x93bea0 as *mut u8,
        delay_state: 0x96b400 as *mut u8,
        availability: 0x95b098 as *mut u8,
        buffer: allocate(0x1000),
    };
    let result = register(&descriptor);
    store(0x954d68, result);
    if result != 0 { registration_error(); }
    setup_service();
    setup_zero(0);
}

unsafe fn clear_command() {
    site!("c231ec");
    store(0x944580, 0);
    store(0x944584, 0);
    write_volatile(0x944588 as *mut u16, 0);
}

unsafe fn accept_received(held: bool) -> bool {
    let command = word(0x944580);
    if command & !0xc0 < 0x600 {
        write_volatile(0x944588 as *mut u16, (command & 0xc0) as u16);
        store(0x944580, command & !0xc0);
        low_command();
    }
    command_service();
    let allowed = predicate_a() == 0 || predicate_b() == 0 || matches!(word(0x944580), 6 | 0x204 | 0x210 | 0x11 | 0xe);
    let suppressed = suppression() == 1;
    if held {
        if word(0x944580).wrapping_sub(0x837) <= 1 {
            held_command();
        } else {
            held_clear(0);
            return false;
        }
    }
    allowed && !suppressed
}

unsafe fn pending() -> bool {
    site!("c23094");
    if observed!("c230a0", word(0x950148)) == 1 {
        pending_first();
        store(0x950148, 0);
    } else if { site!("c230b4"); observed!("c230c0", word(0x951528)) } == 1 {
        pending_second();
        image4_pending(0);
    } else if observed!("c230d8", pending_predicate()) == 1 {
        pending_third();
    } else if observed!("c230f4", word(0x950098)) != 0 {
        pending_priority();
    } else if observed!("c23104", deferred_first()) == 1 {
        site!("c23114");
        store(0x94f5e0, 2);
    } else if observed!("c2311c", deferred_second()) == 1 {
        site!("c2312c");
        store(0x94f5e0, 1);
    } else {
        return false;
    }
    true
}

#[no_mangle]
#[link_section = ".text.actor"]
pub unsafe extern "C" fn xm1_task1_actor() -> ! {
    site!("c22f64");
    setup();
    controller_setup();
    site!("c22f74");
    loop {
        site!("c22f84");
        loop_service();
        execution(0, 0, 0, 0, 0);
        store(0x94f5e0, 0);
        let held = read_volatile(0x949dd4 as *const u8) != 0;
        site!("c22fb4");
        let dispatch = if !held && pending() {
            true
        } else {
            let request = Receive {
                operation: ReceiveOperation::Block,
                command: 0x944580 as *mut u32,
                argument: 0x944584 as *mut u32,
                payload: 0x94458c as *mut u32,
            };
            let status = if held {
                let status = receive(&request);
                site!("c22fd8");
                status
            } else {
                let status = receive(&request);
                site!("c23150");
                status
            };
            status == 0 && accept_received(held)
        };
        if dispatch {
            site!("c231fc");
            after_first();
            after_second();
            after_third();
            if word(0x944580) == 9 && read_volatile(0x944588 as *const u16) == 0 {
                diagnostic();
            }
            let command = word(0x944580);
            if (0x200..=0x203).contains(&command) { store(0x95200c, command); }
        } else {
            clear_command();
        }
        pump();
        site!("c23278");
    }
}
