//! Radio object construction at 0x012af120/0x012af3ac and destruction at 0x012af30c.
//! Wrapper policies are confirmed at 0x007291d8, 0x007293b0, 0x00729768 and 0x00728804.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CoreClass { Primary = 1, Secondary = 2 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cursor { Semaphore, Event, Task }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelId(u32);
impl KernelId { pub const fn wire(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemaphoreHandle(u16);
impl SemaphoreHandle { pub const fn abi(self) -> i32 { self.0 as i32 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventHandle(u16);
impl EventHandle { pub const fn abi(self) -> i32 { self.0 as i32 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskHandle(u16);
impl TaskHandle { pub const fn abi(self) -> i32 { self.0 as i32 } }
pub struct TaskStarted { task: TaskHandle, kind: TaskKind }
impl TaskStarted {
    pub const fn task(&self) -> TaskHandle { self.task }
    pub const fn kind(&self) -> TaskKind { self.kind }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectError { Kernel(ObjectStatus), InvalidCore, WrongOwner, Exhausted, Busy, InvalidMapping, MissingTask }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemaphoreHandles { pub global: SemaphoreHandle, pub adapter: SemaphoreHandle }
#[derive(Clone, Copy)]
#[repr(C)]
pub struct SemaphoreRecord { pub attributes: u32, pub initial: u32, pub maximum: u32 }
#[derive(Clone, Copy)]
#[repr(C)]
pub struct EventRecord { pub attributes: u32, pub initial: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind { Driver, Supplicant }
impl TaskKind {
    pub const fn stack_length(self) -> usize { match self { Self::Driver => 2048, Self::Supplicant => 8192 } }
    pub const fn priority(self) -> u32 { match self { Self::Driver => 3, Self::Supplicant => 10 } }
}
#[derive(Clone, Copy)]
pub struct TaskRecord { pub kind: TaskKind, pub context: TaskContext, pub stack: TaskStackAddress }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskContext(u32);
impl TaskContext {
    /// # Safety
    /// The mapping names the root-owned wireless runtime for the entire task lifetime.
    pub unsafe fn from_mapping(address: u32) -> Result<Self, ObjectError> {
        if address == 0 || address & 3 != 0 { Err(ObjectError::InvalidMapping) } else { Ok(Self(address)) }
    }
    pub const fn address(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskStackAddress(u32);
impl TaskStackAddress { pub const fn address(self) -> u32 { self.0 } }
pub struct TaskStack { address: TaskStackAddress, bytes: &'static mut [u8], kind: TaskKind }
impl TaskStack {
    /// # Safety
    /// `address` maps `bytes` in task RAM and remains mapped until task deletion completes.
    pub unsafe fn from_mapping(kind: TaskKind, address: u32, bytes: &'static mut [u8]) -> Result<Self, ObjectError> {
        if address == 0 || address & 7 != 0 || bytes.len() != kind.stack_length() || address.checked_add(bytes.len() as u32).is_none() { return Err(ObjectError::InvalidMapping); }
        Ok(Self { address: TaskStackAddress(address), bytes, kind })
    }
    pub fn into_bytes(self) -> &'static mut [u8] { self.bytes }
}

pub struct TaskStartError { pub error: ObjectError, pub stack: Option<TaskStack> }
pub trait ObjectKernel {
    fn core(&self) -> Result<CoreClass, ObjectError>;
    fn current_task(&self) -> Result<TaskHandle, ObjectError>;
    fn cursor(&self, kind: Cursor) -> i32;
    fn set_cursor(&mut self, kind: Cursor, value: i32);
    fn semaphore_class(&self, slot: u16) -> u32;
    fn set_semaphore_class(&mut self, slot: u16, class: u32);
    fn mask_interrupts(&mut self) -> u32;
    fn restore_interrupts(&mut self, saved: u32);
    fn create_semaphore(&mut self, id: KernelId, record: SemaphoreRecord) -> ObjectStatus;
    fn delete_semaphore(&mut self, id: KernelId) -> ObjectStatus;
    fn create_event(&mut self, id: KernelId, record: EventRecord) -> ObjectStatus;
    fn delete_event(&mut self, id: KernelId) -> ObjectStatus;
    fn set_event(&mut self, id: KernelId, bits: u32) -> ObjectStatus;
    fn create_task(&mut self, id: KernelId, record: TaskRecord) -> ObjectStatus;
    fn activate_task(&mut self, id: KernelId) -> ObjectStatus;
    fn suspend_task(&mut self, id: KernelId) -> ObjectStatus;
    fn terminate_task(&mut self, id: KernelId) -> ObjectStatus;
    fn delete_task(&mut self, id: KernelId) -> ObjectStatus;
}

pub struct RadioObjects {
    adapter: Option<SemaphoreHandle>,
    global: Option<SemaphoreHandle>,
    event: Option<EventHandle>,
    task: Option<TaskHandle>,
    stack: Option<TaskStack>,
    shutdown_requested: bool,
    kind: TaskKind,
}
impl Default for RadioObjects { fn default() -> Self { Self::new() } }
impl RadioObjects {
    pub const fn new() -> Self { Self::with_kind(TaskKind::Driver) }
    pub const fn supplicant() -> Self { Self::with_kind(TaskKind::Supplicant) }
    const fn with_kind(kind: TaskKind) -> Self { Self { adapter: None, global: None, event: None, task: None, stack: None, shutdown_requested: false, kind } }
    pub fn initialize(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), ObjectError> {
        if self.adapter.is_some() || self.global.is_some() || self.event.is_some() { return Err(ObjectError::Busy); }
        if self.kind == TaskKind::Driver { self.adapter = Some(create_semaphore(kernel)?); }
        self.global = Some(create_semaphore(kernel)?);
        self.event = Some(create_event(kernel)?);
        Ok(())
    }
    pub const fn handles(&self) -> Option<SemaphoreHandles> {
        match (self.global, self.adapter, self.event) {
            (Some(global), Some(adapter), Some(_)) => Some(SemaphoreHandles { global, adapter }),
            _ => None,
        }
    }
    pub const fn task(&self) -> Option<TaskHandle> { self.task }
    pub const fn event(&self) -> Option<EventHandle> { self.event }
    pub fn observe_task_started(&self, kernel: &impl ObjectKernel) -> Result<Option<TaskStarted>, ObjectError> {
        let Some(task) = self.task else { return Ok(None); };
        Ok((kernel.current_task()? == task).then_some(TaskStarted { task, kind: self.kind }))
    }
    pub fn start_task(&mut self, kernel: &mut impl ObjectKernel, context: TaskContext, mut stack: TaskStack) -> Result<TaskHandle, TaskStartError> {
        if self.global.is_none() || self.event.is_none() || self.task.is_some() || stack.kind != self.kind { return Err(TaskStartError { error: ObjectError::Busy, stack: Some(stack) }); }
        stack.bytes.fill(0);
        let mut candidate = kernel.cursor(Cursor::Task);
        let mut last = ObjectStatus(-34);
        while candidate > 104 {
            let slot = candidate as u16;
            let id = KernelId(0x1_0000 | u32::from(slot));
            last = kernel.create_task(id, TaskRecord { kind: self.kind, context, stack: stack.address });
            if last.0 == 0 {
                let saved = kernel.mask_interrupts();
                kernel.set_cursor(Cursor::Task, candidate - 1);
                kernel.restore_interrupts(saved);
                self.task = Some(TaskHandle(slot));
                self.stack = Some(stack);
                let activated = kernel.activate_task(id);
                return if activated.0 == 0 { Ok(TaskHandle(slot)) } else { Err(TaskStartError { error: ObjectError::Kernel(activated), stack: None }) };
            }
            candidate -= 1;
        }
        Err(TaskStartError { error: ObjectError::Kernel(last), stack: Some(stack) })
    }
    pub fn request_shutdown(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), ObjectError> {
        let event = self.event.ok_or(ObjectError::MissingTask)?;
        if self.task.is_none() { return Err(ObjectError::MissingTask); }
        self.shutdown_requested = true;
        status(kernel.set_event(KernelId(0x1_0000 | event.0 as u32), 1))
    }
    pub const fn shutdown_requested(&self) -> bool { self.shutdown_requested }
    pub fn delete_task(&mut self, kernel: &mut impl ObjectKernel) -> Result<Option<TaskStack>, ObjectError> {
        let Some(task) = self.task else { return Ok(None); };
        let id = KernelId(0x1_0000 | task.0 as u32);
        let _ = kernel.suspend_task(id);
        let _ = kernel.terminate_task(id);
        status(kernel.delete_task(id))?;
        publish_cursor(kernel, Cursor::Task, task.abi());
        self.task = None;
        Ok(self.stack.take())
    }
    pub fn destroy(&mut self, kernel: &mut impl ObjectKernel) -> Result<(), ObjectError> {
        if self.task.is_some() { return Err(ObjectError::Busy); }
        if let Some(event) = self.event {
            status(kernel.delete_event(KernelId(0x1_0000 | event.0 as u32)))?;
            publish_cursor(kernel, Cursor::Event, event.abi());
            self.event = None;
        }
        if let Some(global) = self.global { delete_semaphore(kernel, global)?; self.global = None; }
        if let Some(adapter) = self.adapter { delete_semaphore(kernel, adapter)?; self.adapter = None; }
        self.shutdown_requested = false;
        Ok(())
    }
}

fn status(status: ObjectStatus) -> Result<(), ObjectError> { if status.0 == 0 { Ok(()) } else { Err(ObjectError::Kernel(status)) } }
fn semaphore_id(kernel: &impl ObjectKernel, slot: u16) -> KernelId {
    KernelId(u32::from(slot) | if slot <= 169 { kernel.semaphore_class(slot) << 16 } else { 0x1_0000 })
}
fn create_semaphore(kernel: &mut impl ObjectKernel) -> Result<SemaphoreHandle, ObjectError> {
    let class = kernel.core()?;
    let mut candidate = kernel.cursor(Cursor::Semaphore);
    let mut last = ObjectStatus(-34);
    while candidate > 0 {
        let slot = candidate as u16;
        last = kernel.create_semaphore(semaphore_id(kernel, slot), SemaphoreRecord { attributes: 0, initial: 1, maximum: 1 });
        if last.0 == 0 {
            let saved = kernel.mask_interrupts();
            kernel.set_semaphore_class(slot, class as u32);
            kernel.set_cursor(Cursor::Semaphore, candidate - 1);
            kernel.restore_interrupts(saved);
            return Ok(SemaphoreHandle(slot));
        }
        candidate -= 1;
    }
    Err(ObjectError::Kernel(last))
}
fn delete_semaphore(kernel: &mut impl ObjectKernel, semaphore: SemaphoreHandle) -> Result<(), ObjectError> {
    let class = kernel.core()?;
    if kernel.semaphore_class(semaphore.0) != class as u32 { return Err(ObjectError::WrongOwner); }
    status(kernel.delete_semaphore(semaphore_id(kernel, semaphore.0)))?;
    let saved = kernel.mask_interrupts();
    kernel.set_semaphore_class(semaphore.0, 0);
    if semaphore.abi() > kernel.cursor(Cursor::Semaphore) { kernel.set_cursor(Cursor::Semaphore, semaphore.abi()); }
    kernel.restore_interrupts(saved);
    Ok(())
}
fn create_event(kernel: &mut impl ObjectKernel) -> Result<EventHandle, ObjectError> {
    kernel.core()?;
    let mut candidate = kernel.cursor(Cursor::Event);
    let mut last = ObjectStatus(-34);
    while candidate > 104 {
        let slot = candidate as u16;
        last = kernel.create_event(KernelId(0x1_0000 | u32::from(slot)), EventRecord { attributes: 4, initial: 0 });
        if last.0 == 0 {
            let saved = kernel.mask_interrupts();
            kernel.set_cursor(Cursor::Event, candidate - 1);
            kernel.restore_interrupts(saved);
            return Ok(EventHandle(slot));
        }
        candidate -= 1;
    }
    Err(ObjectError::Kernel(last))
}
fn publish_cursor(kernel: &mut impl ObjectKernel, kind: Cursor, handle: i32) {
    let saved = kernel.mask_interrupts();
    if handle > kernel.cursor(kind) { kernel.set_cursor(kind, handle); }
    kernel.restore_interrupts(saved);
}

#[cfg(target_arch = "arm")]
#[path = "vendor_wireless_boot_objects_native.rs"]
pub mod native;
