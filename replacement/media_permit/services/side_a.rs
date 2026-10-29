#![no_std]

#[path = "src/model.rs"]
mod model;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

fn event_class(message_id: u16) -> u32 {
    match message_id {
        0x1331 => 0,
        0x1332 => 1,
        0x138b | 0x1334 | 0x138c => 2,
        0x1335 => 3,
        0x1339 => 4,
        0x1336 => 5,
        0x1337 => 6,
        0x134c => 7,
        0x9b07 => 8,
        0x1338 => 9,
        _ => 10,
    }
}

#[no_mangle]
pub extern "C" fn rb_mp_handback(_class: u32) {
    let class = event_class(model::read_u16(model::MIRROR));
    model::write_u32(model::TRACE_HANDBACK_CLASS, class);
    model::bump(model::TRACE_HANDBACK_COUNT);
}
