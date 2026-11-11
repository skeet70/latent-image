#![cfg(target_arch = "arm")]
use core::ptr::{read_volatile, write_volatile};
use crate::record_task50::{self as task, Event, Job, JobPolicy, JobState, Opcode, ReceiveStatus, Synchronization, Task50Io};
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
use crate::shutdown_receive::{PayloadAddress, ReceiveDescriptor, ReceiveTimeout};
use crate::shutdown_rtos::NativeShutdownRtos;
const RUNTIME: u32 = 0x00942ab0;
const JOBS: u32 = 0x0095dc24;
const TASK: u32 = 0x00972e94;
const DESCRIPTORS: u32 = 0x00819fe4;
fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn half(address: u32) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
fn runtime(offset: u32) -> u32 { word(RUNTIME + offset) }
fn job_address(job: Job) -> u32 { JOBS.wrapping_add((job.0 as u32).wrapping_mul(44)) }
fn descriptor(job: Job, offset: u32) -> u32 { DESCRIPTORS.wrapping_add((job.0 as u32).wrapping_mul(12)).wrapping_add(offset) }
unsafe extern "C" {
    fn xm1_power_task50_dispatch_event(event: u32, argument: i32);
    fn xm1_power_task50_receive_failure();
    fn xm1_power_task50_synchronize_focus(first: u32, position: i32, mode: u32);
    fn xm1_power_task50_synchronize_focus_current();
    fn xm1_power_task50_synchronize_iris();
    fn xm1_power_task50_actuator_snapshot(output: *mut u32);
    fn xm1_power_profile_diagnostic_747b50() -> i32;
}
pub struct NativeTask50;
impl Task50Io for NativeTask50 {
    fn opcode(&self) -> Opcode { Opcode(half(TASK + 4)) }
    fn job_policy(&self, job: Job) -> JobPolicy { if word(descriptor(job, 4)) == 0 { JobPolicy::ReleaseAfterMessage } else { JobPolicy::RetainUntilCompletion } }
    fn job_state(&self, job: Job) -> JobState { if job == Job(0x21) { return JobState(-1); } let address = word(descriptor(job, 8)); JobState(if address == 0 { -1 } else { word(address) as i32 }) }
    fn job_active(&self, job: Job) -> bool { word(job_address(job)) != 0 }
    fn set_job_active(&mut self, job: Job, active: bool) { store(job_address(job), u32::from(active)); }
    fn job_pending(&self, job: Job) -> bool { word(job_address(job) + 4) != 0 }
    fn set_job_pending(&mut self, job: Job, pending: bool) { store(job_address(job) + 4, u32::from(pending)); }
    fn active_job(&self) -> Job { Job(runtime(0xbc) as i32) }
    fn set_active_job(&mut self, job: Job) { store(RUNTIME + 0xbc, job.0 as u32); }
    fn copy_received_to_job(&mut self, job: Job) {
        let source = word(TASK + 12);
        let length = word(TASK + 8);
        let destination = job_address(job) + 8;
        for offset in 0..length { unsafe { write_volatile((destination + offset) as *mut u8, read_volatile((source + offset) as *const u8)); } }
    }
    fn receive_failure(&mut self, _status: ReceiveStatus) { unsafe { xm1_power_task50_receive_failure(); } }
    fn synchronization(&self, event: Event) -> Option<Synchronization> {
        match event {
            Event(0x33) if word(JOBS + 0x1e4) != 0 => match word(JOBS + 0x200) {
                mode @ 5..=8 => Some(Synchronization { first: matches!(mode, 6 | 7), position: half(JOBS + 0x1f0) as i16, mode: word(JOBS + 0x1f4) }),
                _ => None,
            },
            Event(0x34) if word(JOBS + 0x210) != 0 && word(JOBS + 0x230).wrapping_sub(5) < 4 => Some(Synchronization { first: false, position: half(JOBS + 0x21c) as i16, mode: word(JOBS + 0x224) }),
            _ => None,
        }
    }
    fn synchronize_focus(&mut self, synchronization: Option<Synchronization>) { unsafe { match synchronization { Some(value) => xm1_power_task50_synchronize_focus(u32::from(value.first), i32::from(value.position), value.mode), None => xm1_power_task50_synchronize_focus_current() } } }
    fn iris_in_progress(&self) -> bool { runtime(0xe4) != 0 }
    fn synchronize_iris(&mut self) { unsafe { xm1_power_task50_synchronize_iris(); } }
    fn notify_focus_status(&mut self) {
        if runtime(0xf8) != 1 { return; }
        let callback = runtime(0xc4);
        if callback == 0 { return; }
        let flags = if runtime(0xf4) == 1 { 3 } else { 1 } | if runtime(0xfc) == 1 { 4 } else { 0 };
        let callback: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(flags); }
    }
    fn notify_position(&mut self) {
        if runtime(0xcc) != 1 { return; }
        let callback = runtime(0xc8);
        if callback == 0 { return; }
        let first = runtime(0xd4) as i32;
        let value = runtime(0x104);
        let mode = if value == 0 { 0 } else if word(0x0081a174) == value { 1 } else if word(0x0081a178) == value { 2 } else { 3 };
        let callback: unsafe extern "C" fn(i32, i32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(first, mode); }
    }
    fn notify_iris_status(&mut self) {
        if runtime(0xfc) != 1 { return; }
        let callback = runtime(0xc4);
        if callback == 0 { return; }
        let flags = if runtime(0xf4) == 1 { 6 } else { 4 } | if runtime(0xf8) == 1 { 1 } else { 0 };
        let callback: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(flags); }
    }
    fn dispatch(&mut self, event: Event, argument: JobState) { unsafe { xm1_power_task50_dispatch_event(event.0, argument.0); } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task50_post(opcode: u32, payload: *const u32, word_count: u32) -> u32 {
    let payload = if word_count == 0 { &[][..] } else { core::slice::from_raw_parts(payload.cast::<u8>(), (word_count * 4) as usize) };
    let mut rtos = NativeShutdownRtos;
    let mut sender = rtos.current_task();
    if sender.0 == 0 { sender.0 = 0xff; }
    match rtos.post(ServiceMessage { operation: RequestId(opcode as u16), sender, destination: QueueId(50), payload }) { Ok(()) => 0, Err(status) => status.0 as u32 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task50_entry(_argument: *const u32) -> ! {
    for index in 0..17 { write_volatile((0x00957060 + index * 2) as *mut u16, 0); }
    let descriptor = &mut *((TASK + 4) as *mut ReceiveDescriptor);
    loop {
        descriptor.payload = PayloadAddress(TASK + 16);
        task::before_receive(&mut NativeTask50);
        let received = NativeShutdownRtos.receive(QueueId(50), descriptor, ReceiveTimeout::Forever).map_err(|status| ReceiveStatus(status.0));
        task::consume(&mut NativeTask50, received);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_task50_adjustment(first: *mut u32, second: *mut u16) {
    let mut snapshot = [0u32; 2];
    xm1_power_task50_actuator_snapshot(snapshot.as_mut_ptr());
    let adjustment = if snapshot[0] == 0 { task::adjusted_position(snapshot[1] & 0xff, (snapshot[1] >> 8) as u8, xm1_power_profile_diagnostic_747b50()) } else { task::Adjustment { position: 0, fraction: 0 } };
    if !first.is_null() { write_volatile(first, adjustment.position); }
    if !second.is_null() { write_volatile(second, adjustment.fraction); }
}
