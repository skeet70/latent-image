//! Task 99 effects exposed after coherent state publication.

use crate::control_device::{CalibrationProfile,KeyPublication};
use crate::control_types::*;

enum Effect {
    Complete {callback:CallbackToken,status:CompletionStatus,data:[u8;9],length:usize},
    Notify(WaitTarget),KeyRelease(KeyEvent),Key(KeyPublication),Diagnostic(u32,u32),
}
pub struct ControlOutbox {effects:[Option<Effect>;40],count:usize}
impl ControlOutbox {
    pub const fn new()->Self {Self {effects:[const {None};40],count:0}}
    fn push(&mut self,effect:Effect) {
        assert!(self.count<self.effects.len(),"task99 effect capacity");
        self.effects[self.count]=Some(effect);self.count+=1;
    }
    pub fn flush(&mut self,io:&mut impl ControlIo) {
        for index in 0..self.count {
            match self.effects[index].take().expect("pending effect") {
                Effect::Complete {callback,status,data,length}=>io.complete(callback,status,&data[..length]),
                Effect::Notify(target)=>io.notify(target),Effect::KeyRelease(key)=>io.key_release(key),
                Effect::Key(key)=>io.publish_key(key),Effect::Diagnostic(category,event)=>io.diagnostic(category,event),
            }
        }
        self.count=0;
    }
}
pub struct PublicationIo<'a,I> {pub io:&'a mut I,pub outbox:&'a mut ControlOutbox}
impl<I:ControlIo> ControlServices for PublicationIo<'_,I> {
    fn delay_ticks(&mut self,ticks:TickCount) {self.io.delay_ticks(ticks);}
    fn post(&mut self,message:ControlMessage)->Result<(),ControlError> {self.io.post(message)}
    fn complete(&mut self,callback:CallbackToken,status:CompletionStatus,data:&[u8]) {
        assert!(data.len()<=9,"task99 completion payload");
        let mut copied=[0;9];copied[..data.len()].copy_from_slice(data);
        self.outbox.push(Effect::Complete {callback,status,data:copied,length:data.len()});
    }
    fn notify(&mut self,target:WaitTarget) {self.outbox.push(Effect::Notify(target));}
    fn key_release(&mut self,key:KeyEvent) {self.outbox.push(Effect::KeyRelease(key));}
    fn diagnostic(&mut self,category:u32,event:u32) {self.outbox.push(Effect::Diagnostic(category,event));}
    fn start_timer(&mut self,ticks:TickCount,repeating:bool,callback:ControlTimer)->TimerToken {self.io.start_timer(ticks,repeating,callback)}
    fn cancel_timer(&mut self,timer:TimerToken) {self.io.cancel_timer(timer);}
    fn set_detection_callback(&mut self,edge:DetectionEdge,enabled:bool) {self.io.set_detection_callback(edge,enabled);}
    fn reset_gpio(&mut self) {self.io.reset_gpio();}
}
impl<I:ControlIo> ControlPower for PublicationIo<'_,I> {
    fn set_detection_mode(&mut self,mode:DetectionMode) {self.io.set_detection_mode(mode);}
    fn detection_output(&mut self,mode:DetectionMode,completion:Option<DetectionCompletion>) {self.io.detection_output(mode,completion);}
}
impl<I:ControlIo> ControlProfiles for PublicationIo<'_,I> {fn calibration_profiles(&self)->&[CalibrationProfile] {self.io.calibration_profiles()}}
impl<I:ControlIo> KeyPublicationSink for PublicationIo<'_,I> {fn publish_key(&mut self,key:KeyPublication) {self.outbox.push(Effect::Key(key));}}
impl<I:ControlIo> ControlIo for PublicationIo<'_,I> {
    fn read_mmio(&mut self,address:MmioAddress)->u32 {self.io.read_mmio(address)}
    fn write_mmio(&mut self,address:MmioAddress,value:u32) {self.io.write_mmio(address,value);}
    fn update_mmio(&mut self,address:MmioAddress,clear:u32,set:u32) {self.io.update_mmio(address,clear,set);}
}
