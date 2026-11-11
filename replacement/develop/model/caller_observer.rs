#![no_std]

use core::ptr::{addr_of_mut, write_volatile, read_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ContextAddress(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct OperationCode(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct StepResult(u32);

#[repr(C)]
struct CallbackEvent {
    context: ContextAddress,
    operation: OperationCode,
    result: StepResult,
}

#[repr(C)]
struct CallbackJournal {
    count: u32,
    overflow: u32,
    events: [CallbackEvent; 16],
}

#[no_mangle]
#[link_section = ".text.xm1_develop_proof_caller_callback"]
pub unsafe extern "C" fn xm1_develop_proof_caller_callback(context: u32, operation: u32, result: u32) {
    let journal = 0x009a3800 as *mut CallbackJournal;
    let count = unsafe { read_volatile(addr_of_mut!((*journal).count)) };
    if count < 16 {
        let event = unsafe { addr_of_mut!((*journal).events).cast::<CallbackEvent>().add(count as usize) };
        unsafe {
            write_volatile(addr_of_mut!((*event).context), ContextAddress(context));
            write_volatile(addr_of_mut!((*event).operation), OperationCode(operation));
            write_volatile(addr_of_mut!((*event).result), StepResult(result));
            write_volatile(addr_of_mut!((*journal).count), count + 1);
        }
    } else {
        unsafe { write_volatile(addr_of_mut!((*journal).overflow), 1) };
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
