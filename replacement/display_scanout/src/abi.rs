use core::ptr::{read_volatile,write_volatile};
use calibration_liveview::{CalibrationByte,CalibrationOffset,InitializedCalibrationBase};
use crate::{Bank,Engine,EngineState,Error,Hardware,Register,commit,scheduler,shadow,timing};
use crate::{layout as stock,publication_edge as publish};

#[cfg(not(feature = "library"))]
extern "C" {
    fn scanout_enter_critical()->u32;
    fn scanout_leave_critical(token:u32);
    fn scanout_yield(value:u32);
    fn scanout_irq_configure(source:u32,handler:u32,handle:*mut u32)->i32;
    fn scanout_irq_release(handle:u32)->i32;
}
#[cfg(feature = "library")]
mod owned_primitives {
    unsafe extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(token: u32);
        fn xm1_power_task_delay(ticks: u32) -> i32;
        fn xm1_power_timer_irq_attach(source: u32, handler: unsafe extern "C" fn(), output: *mut u32) -> i32;
        fn xm1_power_timer_release_handle(handle: u32) -> i32;
    }
    pub(super) unsafe fn scanout_enter_critical() -> u32 { unsafe { xm1_power_interrupt_lock() } }
    pub(super) unsafe fn scanout_leave_critical(token: u32) { unsafe { xm1_power_interrupt_unlock(token); } }
    pub(super) unsafe fn scanout_yield(value: u32) { unsafe { let _ = xm1_power_task_delay(value); } }
    pub(super) unsafe fn scanout_irq_configure(source: u32, handler: u32, output: *mut u32) -> i32 {
        let handler: unsafe extern "C" fn() = unsafe { core::mem::transmute(handler as usize) };
        unsafe { xm1_power_timer_irq_attach(source, handler, output) }
    }
    pub(super) unsafe fn scanout_irq_release(handle: u32) -> i32 { unsafe { xm1_power_timer_release_handle(handle) } }
}
#[cfg(feature = "library")]
use owned_primitives::*;
struct Device { immediate:Option<Engine>,force_clear:Option<Engine> }
impl Device {fn plain()->Self{Self{immediate:None,force_clear:None}}}
fn register_address(register:Register)->usize{let base=match register.bank{Bank::Scanout=>0xfff6b000,Bank::Timing=>0xfff6a000,Bank::Formatter=>0xfff69000};base+register.engine.index()*0x3000+usize::from(register.offset)}
impl Hardware for Device {
    fn read(&mut self,register:Register)->u32{unsafe{stock::word(register_address(register))}}
    fn write(&mut self,register:Register,value:u32){unsafe{if register.bank==Bank::Timing{if self.immediate==Some(register.engine)&&register.offset==0x34{stock::put_word(stock::timing_base(register.engine)+0x50,value);}if self.force_clear==Some(register.engine)&&register.offset==0{stock::put_word(stock::timing_base(register.engine)+0x14,0);}}stock::put_word(register_address(register),value);}}
    fn enter_critical(&mut self)->u32{unsafe{scanout_enter_critical()}}
    fn leave_critical(&mut self,token:u32){unsafe{scanout_leave_critical(token);}}
    fn yield_zero(&mut self){unsafe{scanout_yield(0);}}
}
fn handler_address(handler:scheduler::Handler)->u32{match handler{scheduler::Handler::TimingZero=>xm1_scanout_isr_zero as *const () as u32,scheduler::Handler::TimingOne=>xm1_scanout_isr_one as *const () as u32}}
impl scheduler::Irq for Device {
    fn configure(&mut self,source:scheduler::IrqSource,handler:scheduler::Handler)->Result<scheduler::Handle,scheduler::IrqError>{let mut handle=0;let result=unsafe{scanout_irq_configure(source as u32,handler_address(handler),&mut handle)};if result==0{Ok(scheduler::Handle(handle))}else{Err(scheduler::IrqError(result))}}
    fn release(&mut self,handle:scheduler::Handle)->Result<(),scheduler::IrqError>{let result=unsafe{scanout_irq_release(handle.0)};if result==0{Ok(())}else{Err(scheduler::IrqError(result))}}
}
struct StockScheduler;
fn irq_cell(handler:scheduler::Handler)->usize{match handler{scheduler::Handler::TimingZero=>0x917ed4,scheduler::Handler::TimingOne=>0x917ed0}}
fn slot_cell(list:scheduler::List,slot:scheduler::SlotIndex)->usize{(match list{scheduler::List::Persistent=>0x917ee4,scheduler::List::OneShot=>0x917f24})+slot.index()*16}
impl scheduler::SchedulerState for StockScheduler {
    fn irq_handle(&self,handler:scheduler::Handler)->scheduler::Handle{scheduler::Handle(unsafe{stock::word(irq_cell(handler))})}
    fn set_irq_handle(&self,handler:scheduler::Handler,value:scheduler::Handle){unsafe{stock::put_word(irq_cell(handler),value.0);}}
    fn next_handle(&self)->scheduler::Handle{scheduler::Handle(unsafe{stock::word(0x917ed8)})}
    fn set_next_handle(&self,value:scheduler::Handle){unsafe{stock::put_word(0x917ed8,value.0);}}
    fn activity(&self)->scheduler::Activity{if unsafe{stock::word(0x917edc)}==0{scheduler::Activity::Inactive}else{scheduler::Activity::Active}}
    fn set_activity(&self,value:scheduler::Activity){unsafe{stock::put_word(0x917edc,match value{scheduler::Activity::Inactive=>0,scheduler::Activity::Active=>1});}}
    fn phase_count(&self)->scheduler::PhaseCount{scheduler::PhaseCount(unsafe{stock::word(0x917ee0)})}
    fn set_phase_count(&self,value:scheduler::PhaseCount){unsafe{stock::put_word(0x917ee0,value.0);}}
    fn slot_handle(&self,list:scheduler::List,slot:scheduler::SlotIndex)->scheduler::Handle{scheduler::Handle(unsafe{stock::word(slot_cell(list,slot))})}
    fn set_slot_handle(&self,list:scheduler::List,slot:scheduler::SlotIndex,value:scheduler::Handle){unsafe{stock::put_word(slot_cell(list,slot),value.0);}}
    fn slot_order(&self,list:scheduler::List,slot:scheduler::SlotIndex)->scheduler::Order{scheduler::Order(unsafe{stock::word(slot_cell(list,slot)+4)})}
    fn set_slot_order(&self,list:scheduler::List,slot:scheduler::SlotIndex,value:scheduler::Order){unsafe{stock::put_word(slot_cell(list,slot)+4,value.0);}}
    fn slot_callback(&self,list:scheduler::List,slot:scheduler::SlotIndex)->Option<scheduler::CallbackId>{scheduler::CallbackId::new(unsafe{stock::word(slot_cell(list,slot)+8)})}
    fn set_slot_callback(&self,list:scheduler::List,slot:scheduler::SlotIndex,value:Option<scheduler::CallbackId>){unsafe{stock::put_word(slot_cell(list,slot)+8,value.map_or(0,scheduler::CallbackId::get));}}
    fn slot_phase(&self,list:scheduler::List,slot:scheduler::SlotIndex)->scheduler::SlotPhase{scheduler::SlotPhase(unsafe{stock::word(slot_cell(list,slot)+12)})}
    fn set_slot_phase(&self,list:scheduler::List,slot:scheduler::SlotIndex,value:scheduler::SlotPhase){unsafe{stock::put_word(slot_cell(list,slot)+12,value.0);}}
}
struct Invoke;
impl scheduler::Callbacks<Device,StockScheduler> for Invoke {
    fn invoke(&mut self,callback:scheduler::CallbackId,_scheduler:&scheduler::Scheduler<StockScheduler>,_hardware:&mut Device){unsafe{let call:unsafe extern "C" fn()=core::mem::transmute(callback.get() as usize);call();}}
}
struct RegistrationOutput{packet:*mut u32,output:*mut u32}
impl scheduler::RegistrationObserver for RegistrationOutput {
    fn output(&mut self,handle:scheduler::Handle){unsafe{write_volatile(self.output,handle.0);}}
    fn phase(&mut self,phase:scheduler::Phase){if !self.packet.is_null(){unsafe{write_volatile(self.packet.add(3),phase.word());}}}
}
struct Completion;
impl commit::CompletionCallbacks for Completion {
    fn prepared(&mut self,engine:Engine,state:&EngineState){unsafe{publish::prepared(engine,state);}}
    fn call(&mut self,callback:commit::CallbackId,slot:commit::CompletionSlot,engine:Engine,state:&mut EngineState){unsafe{publish::clear_completion(engine,slot);let call:unsafe extern "C" fn()=core::mem::transmute(callback.0 as usize);call();state.runtime=stock::load_runtime(engine);}}
}
struct Registration;
impl commit::Registration for Registration {
    fn register_commit(&mut self,engine:Engine,state:&mut EngineState)->i32{unsafe{
        publish::captured(engine,state);
        let output=(stock::runtime_base(engine)+0x14) as *mut u32;
        let result=match engine{
            Engine::Zero=>{let request=scheduler::RegistrationRequest{callback:scheduler::CallbackId::new(xm1_scanout_commit_zero as *const () as u32),retention:scheduler::Retention::PersistentActive,order:scheduler::Order(1),phase:scheduler::Phase::Zero};let mut handle=scheduler::Handle(0);let mut observer=RegistrationOutput{packet:core::ptr::null_mut(),output};scheduler::Scheduler::new(StockScheduler).register_source_observed(request,&mut handle,selected_handler,&mut Device::plain(),&mut observer).map_or_else(|error|error.stock_code(),|()|0)}
            Engine::One=>scanout_irq_configure(14,xm1_scanout_commit_one as *const () as u32,output),
        };
        state.runtime=stock::load_runtime(engine);result
    }}
}
struct CalibrationInput;
impl timing::CalibrationRead for CalibrationInput {
    fn read_byte(&mut self,offset:CalibrationOffset)->CalibrationByte{unsafe{let pointer=read_volatile(0x96ca50 as *const *mut u8);let cache=InitializedCalibrationBase::from_initialized_base(core::ptr::NonNull::new_unchecked(pointer));cache.read_byte(offset)}}
}
fn selected_handler()->scheduler::Handler{scheduler::Handler::from_selector(unsafe{stock::word(0x942a4c)})}
fn engine(value:u32)->Result<Engine,Error>{Engine::parse(value).ok_or(Error::InvalidArgument)}
fn group(value:u32)->Result<shadow::Group,Error>{shadow::Group::parse(value).ok_or(Error::InvalidArgument)}
fn status(value:Result<(),Error>)->i32{value.map_or_else(Error::stock,|()|0)}
fn request(callback:u32,retention:u32,order:u32,phase:u32)->scheduler::RegistrationRequest{scheduler::RegistrationRequest{callback:scheduler::CallbackId::new(callback),retention:scheduler::Retention::from_stock(retention),order:scheduler::Order(order),phase:scheduler::Phase::from_stock(phase)}}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_item_configuration(e:u32,g:u32,item:u32,value:u32)->i32{status((||{let mut state=stock::StockShadow(engine(e)?);let group=group(g)?;let item=shadow::Item::parse(item).ok_or(Error::InvalidArgument)?;shadow::item_configuration(&mut state,group,item,shadow::ItemConfiguration(value));Ok(())})())}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_range_packet(e:u32,g:u32,item:u32,packet:*const u8)->i32{status((||{let mut state=stock::StockShadow(engine(e)?);let group=group(g)?;let item=shadow::Item::parse(item).ok_or(Error::InvalidArgument)?;let mut bytes=[0;11];for(offset,byte)in bytes.iter_mut().enumerate(){*byte=read_volatile(packet.add(offset));}shadow::range_packet(&mut state,group,item,shadow::RangePacket::parse(&bytes));Ok(())})())}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_range_enabled(e:u32,g:u32,item:u32,value:u32)->i32{status((||{let engine=engine(e)?;if stock::byte(stock::runtime_base(engine)+0x54)==0{return Err(Error::Uninitialized);}let group=group(g)?;let item=shadow::ItemSelection::parse(item).ok_or(Error::InvalidArgument)?;let storage=if stock::byte(0x916610)==0{shadow::MaskStorage::Saved}else{shadow::MaskStorage::Alternate};shadow::range_enabled(&mut stock::StockShadow(engine),group,item,storage,value==1)})())}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_group_options(e:u32,g:u32,packet:*const u32)->i32{status((||{let mut state=stock::StockShadow(engine(e)?);let group=group(g)?;let options=shadow::GroupOptions{control:read_volatile(packet),format:if group==shadow::Group::Second{read_volatile(packet.add(1))}else{0},origin:read_volatile(packet.add(2)),first_selector:read_volatile(packet.add(3)),second_selector:read_volatile(packet.add(4))};shadow::group_options(&mut state,group,options);Ok(())})())}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_blank(e:u32,packet:*const u8)->i32{status((||{let engine=engine(e)?;if stock::byte(stock::runtime_base(engine)+0x54)==0{return Err(Error::Uninitialized);}shadow::blank(&mut stock::StockShadow(engine),shadow::Blanking{selector:read_volatile(packet),color:[read_volatile(packet.add(1)),read_volatile(packet.add(2)),read_volatile(packet.add(3))]})})())}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_commit(e:u32)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let mut state=if stock::word(stock::runtime_base(engine)+12)==1{stock::load_engine(engine)}else{let mut state=EngineState::zero();state.runtime=stock::load_runtime(engine);state};let options=commit::CommitOptions{alternate_masks:stock::byte(0x916610)!=0,reverse_vertical:stock::byte(0x916611)!=0};if state.runtime.pending.0==1{stock::put_word(stock::runtime_base(engine)+12,0);}commit::commit(engine,&mut state,options,&mut Device::plain(),&mut Completion);0}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_commit_zero()->i32{xm1_scanout_commit(0)}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_commit_one()->i32{xm1_scanout_commit(1)}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_capture(e:u32,packet:*const u32)->i32{
    let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let configuration=commit::CaptureConfiguration{mode:commit::CaptureMode(read_volatile(packet)),selector:commit::ConfigurationSelector(read_volatile(packet.add(1))),auxiliary:commit::ConfigurationAuxiliary(read_volatile(packet.add(2)))};let mut state=EngineState::zero();state.runtime=stock::load_runtime(engine);let first=state.runtime.capture_count==0;
    let same=!first&&state.runtime.mode==configuration.mode&&state.runtime.selector==configuration.selector&&state.runtime.auxiliary==configuration.auxiliary;
    if first{publish::reset_runtime(engine,configuration);}
    let result=commit::capture(engine,&mut state,configuration,&mut Device::plain(),&mut Registration);
    if !same{if !first{publish::runtime_configuration(engine,&state.runtime);}stock::put_byte(stock::runtime_base(engine)+0x54,state.runtime.capture_count);}result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_scheduler_register(packet:*mut u32,output:*mut u32)->i32{
    let request=request(read_volatile(packet),read_volatile(packet.add(1)),read_volatile(packet.add(2)),read_volatile(packet.add(3)));let mut handle=scheduler::Handle(0);let mut observer=RegistrationOutput{packet,output};scheduler::Scheduler::new(StockScheduler).register_source_observed(request,&mut handle,selected_handler,&mut Device::plain(),&mut observer).map_or_else(|error|error.stock_code(),|()|0)
}
#[no_mangle]
pub extern "C" fn xm1_scanout_isr_zero(){scheduler::Scheduler::new(StockScheduler).dispatch(scheduler::Handler::TimingZero,&mut Device::plain(),&mut Invoke);}
#[no_mangle]
pub extern "C" fn xm1_scanout_isr_one(){scheduler::Scheduler::new(StockScheduler).dispatch(scheduler::Handler::TimingOne,&mut Device::plain(),&mut Invoke);}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_reset(e:u32)->i32{match engine(e){Ok(engine)=>{timing::reset(engine,&mut Device::plain());0},Err(error)=>error.stock()}}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_wait(e:u32)->i32{match engine(e){Ok(engine)=>{timing::wait_input_ready(engine,&mut Device::plain());0},Err(error)=>error.stock()}}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_timing_flush(e:u32)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let mut state=stock::load_timing(engine);if state.pending.0!=0{stock::put_word(stock::timing_base(engine)+0x10,0);}timing::flush(engine,&mut state,&mut Device{immediate:None,force_clear:Some(engine)}) as i32}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_timing_defaults(e:u32,output:*mut u32)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let mut value=read_volatile(output);timing::acquire_defaults(timing::DefaultSelection(stock::byte(stock::timing_base(engine)+0x1c)),&mut value,&mut CalibrationInput);write_volatile(output,value);0}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_timing_immediate(e:u32,packet:*const u8)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let mut state=stock::load_timing(engine);let selection=timing::DefaultSelection(read_volatile(packet.add(4)));stock::put_byte(stock::timing_base(engine)+0x1c,selection.0);let packet=timing::ImmediatePacket{interval:u16::from_le_bytes([read_volatile(packet),read_volatile(packet.add(1))]),selector:read_volatile(packet.add(2)),flag:read_volatile(packet.add(3)),selection};timing::immediate(engine,&mut state,packet,&mut CalibrationInput,&mut Device{immediate:Some(engine),force_clear:None}) as i32}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_timing_stage(e:u32,packet:*mut u32)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let(Some(mode),Some(route),Some(rate))=(timing::TimingMode::parse(read_volatile(packet)),timing::TimingRoute::parse(read_volatile(packet.add(1))),timing::TimingRate::parse(read_volatile(packet.add(2))))else{return -30;};let mut configuration=timing::TimingConfiguration{mode,route,rate};let mut states=[timing::TimingState::zero(),timing::TimingState::zero()];states[engine.index()]=stock::load_timing(engine);stock::put_word(stock::timing_base(Engine::Zero),engine.index() as u32);stock::put_word(stock::timing_base(Engine::One),engine.index() as u32);stock::put_word(stock::timing_base(engine)+0x10,0);stock::put_word(stock::timing_base(engine)+4,mode.raw());stock::put_word(stock::timing_base(engine)+8,route.raw());stock::put_word(stock::timing_base(engine)+12,rate.raw());if matches!(rate.raw(),0|3){write_volatile(packet.add(2),2);}timing::stage(engine,&mut states,&mut configuration,&mut CalibrationInput);stock::store_staged_timing(engine,&states[engine.index()]);0}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_formatter_flush(e:u32)->i32{let engine=match engine(e){Ok(engine)=>engine,Err(error)=>return error.stock()};let mut state=stock::load_formatter(engine);if state.pending.0!=0{stock::put_word(stock::formatter_base(engine)+0x34,0);}timing::formatter_flush(engine,&mut state,&mut Device::plain());0}
#[no_mangle]
pub unsafe extern "C" fn xm1_scanout_formatter_strobe(e:u32)->i32{match engine(e){Ok(engine)=>{timing::formatter_strobe(engine,&mut Device::plain());0},Err(error)=>error.stock()}}
