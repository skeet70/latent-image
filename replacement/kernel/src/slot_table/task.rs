use crate::{Error, SlotTable, Status, TaskBackend, TaskConfig, TaskFlagPolicy, TaskId};

#[derive(Clone, Copy)]
struct StockTaskEntry(u32);
impl StockTaskEntry {
    unsafe fn call(self, argument: u32) -> i32 {
        let entry: unsafe extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(self.0 as usize) };
        unsafe { entry(argument) }
    }
}

#[repr(C)]
struct Descriptor {
    id: u32,
    name: *const core::ffi::c_char,
    priority: u32,
    stack_bytes: u32,
    entry: extern "C" fn(u32),
    argument: u32,
    flags: u32,
}

impl TaskBackend for SlotTable {
    fn task_create(&self, config: TaskConfig) -> Result<(), Error> {
        let descriptor = Descriptor {
            id: u32::from(config.id.get()),
            name: config.name.as_ptr(),
            priority: config.priority.get(),
            stack_bytes: config.stack.get(),
            entry: config.entry,
            argument: config.argument.0,
            flags: match config.flags {
                TaskFlagPolicy::ClearAfterWait => 0,
                TaskFlagPolicy::PreserveAfterWait => 0xa5f0a5f0,
            },
        };
        Status(unsafe { StockTaskEntry(0x725dd8).call((&descriptor as *const Descriptor) as u32) }).check()
    }

    fn task_delete(&self, id: TaskId) -> Result<(), Error> {
        // The wrapper also releases the zero-based storage reservation at 0x72623c.
        Status(unsafe { StockTaskEntry(0x72612c).call(u32::from(id.get())) }).check()
    }
}
