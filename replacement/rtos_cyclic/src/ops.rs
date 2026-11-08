use crate::id::{own_cyclic_slot, target_context, EncodedId};
use crate::layout::{
    CyclicConfig, CyclicControl, CyclicSlot, Domain, GuestAddress, KernelContext,
};
use crate::machine::Machine;
use crate::status::CyclicStatus;
use crate::types::{
    CyclicArgument, CyclicCreate, CyclicHandler, Expiry, PeriodWord,
};

#[inline(always)]
fn lock<M: Machine>(m: &M, context: KernelContext) {
    loop {
        m.irq_set();
        if m.acquire_lock(context) {
            break;
        }
        m.irq_clear();
    }
}

#[inline(always)]
fn unlock<M: Machine>(m: &M, context: KernelContext) {
    m.release_lock(context);
    m.irq_clear();
}

#[inline(always)]
fn control_of<M: Machine>(m: &M, context: KernelContext, slot: CyclicSlot) -> Option<CyclicControl> {
    let table = m.load_word(context.cyclics());
    if table == 0 {
        return None;
    }
    Some(CyclicControl(
        GuestAddress(table).offset((slot.get() - 1) * CyclicControl::SIZE),
    ))
}

#[inline(always)]
fn config_of<M: Machine>(m: &M, control: CyclicControl) -> CyclicConfig {
    CyclicConfig(GuestAddress(m.load_word(control.config())))
}

/// Arms the control's timer entry at `expiry` under the caller's lock.
#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_schedule_core")]
pub fn schedule<M: Machine>(
    m: &M,
    context: KernelContext,
    control: CyclicControl,
    expiry: Expiry,
) -> Result<(), CyclicStatus> {
    let timer = control.timer();
    m.store_word(timer.offset(0x04), m.load_word(context.cyclic_callback()));
    m.store_word(timer.offset(0x08), control.0 .0);
    if !m.insert_timer(context, timer, expiry.0) {
        return Err(CyclicStatus::Timer);
    }
    m.store_word(control.expiry(), expiry.0);
    Ok(())
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_configure_core")]
pub fn configure<M: Machine>(
    m: &M,
    configuration: GuestAddress,
    controls: GuestAddress,
    count: u32,
    callback: u32,
) {
    m.store_word(configuration.offset(0x60), controls.0);
    m.store_word(configuration.offset(0x64), count);
    m.store_word(configuration.offset(0x6c), callback);
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_initialize_core")]
pub fn initialize<M: Machine>(
    m: &M,
    context: KernelContext,
    configs: GuestAddress,
    controls: GuestAddress,
    count: u32,
) {
    for index in 0..count {
        let config = CyclicConfig(configs.offset(index * CyclicConfig::SIZE));
        let control = CyclicControl(controls.offset(index * CyclicControl::SIZE));
        m.store_word(control.config(), config.0 .0);
        let period = PeriodWord(m.load_word(config.period()));
        let active = m.load_word(config.attributes()) & CyclicConfig::START != 0;
        let _ = m.remove_timer(context, control.timer());
        match (active, period.checked()) {
            (true, Some(_period)) => {
                m.store_word(control.active(), 1);
                if schedule(m, context, control, Expiry(m.load_word(config.phase()))).is_err() {
                    m.store_word(control.active(), 0);
                }
            }
            _ => {
                m.store_word(control.active(), 0);
            }
        }
    }
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_create_core")]
pub fn create<M: Machine>(
    m: &M,
    entry: Domain,
    id: EncodedId,
    request: CyclicCreate,
) -> CyclicStatus {
    let slot = match own_cyclic_slot(id) {
        Ok(slot) => slot,
        Err(status) => return status,
    };
    let config = entry.words().cyclic_config(slot);
    let saved = m.mask_interrupts();
    let status = if m.load_word(config.exists()) != 0 {
        CyclicStatus::AlreadyExists
    } else {
        match request.period.checked() {
            None => CyclicStatus::Param,
            Some(period) => {
                m.store_word(config.attributes(), request.attributes.0);
                m.store_word(config.argument(), request.argument.0);
                m.store_word(config.handler(), request.handler.0);
                m.store_word(config.period(), period.get());
                m.store_word(config.phase(), request.phase.0);
                m.store_word(config.exists(), 1);
                CyclicStatus::Ok
            }
        }
    };
    m.restore_interrupts(saved);
    status
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_delete_core")]
pub fn delete<M: Machine>(m: &M, entry: Domain, id: EncodedId) -> CyclicStatus {
    let slot = match own_cyclic_slot(id) {
        Ok(slot) => slot,
        Err(status) => return status,
    };
    let words = entry.words();
    let config = words.cyclic_config(slot);
    let context = words.context(entry);
    let saved = m.mask_interrupts();
    let status = match control_of(m, context, slot) {
        None => {
            zero_config(m, config);
            CyclicStatus::Ok
        }
        Some(control) => {
            lock(m, context);
            let status = if m.remove_timer(context, control.timer()) {
                m.store_word(control.active(), 0);
                zero_config(m, config);
                CyclicStatus::Ok
            } else {
                CyclicStatus::Timer
            };
            m.release_lock(context);
            status
        }
    };
    m.restore_interrupts(saved);
    status
}

#[inline(always)]
fn zero_config<M: Machine>(m: &M, config: CyclicConfig) {
    m.store_word(config.attributes(), 0);
    m.store_word(config.argument(), 0);
    m.store_word(config.handler(), 0);
    m.store_word(config.period(), 0);
    m.store_word(config.phase(), 0);
    m.store_word(config.exists(), 0);
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_start_core")]
pub fn start<M: Machine>(m: &M, entry: Domain, id: EncodedId) -> CyclicStatus {
    if m.handler_context() != 0 || m.irq_masked() {
        return CyclicStatus::Context;
    }
    let context = match target_context(m, entry, id.class()) {
        Ok(context) => context,
        Err(status) => return status,
    };
    let index = id.index();
    if index <= 0 {
        return CyclicStatus::InvalidIndex;
    }
    let count = m.load_word(context.cyclic_count());
    let slot = match CyclicSlot::new(index as u32, count) {
        Some(slot) => slot,
        None => return CyclicStatus::InvalidIndex,
    };
    let control = match control_of(m, context, slot) {
        Some(control) => control,
        None => return CyclicStatus::InvalidIndex,
    };
    let config = config_of(m, control);
    if m.load_word(config.exists()) == 0 {
        return CyclicStatus::NotCreated;
    }
    lock(m, context);
    let status = match PeriodWord(m.load_word(config.period())).checked() {
        None => CyclicStatus::Param,
        Some(period) => {
            let status = if m.remove_timer(context, control.timer()) {
                m.store_word(control.active(), 1);
                let tick = m.load_word(GuestAddress(m.load_word(context.tick_now())));
                let expiry = Expiry(tick.wrapping_add(period.get()));
                match schedule(m, context, control, expiry) {
                    Ok(()) => CyclicStatus::Ok,
                    Err(status) => status,
                }
            } else {
                CyclicStatus::Timer
            };
            if !status.is_ok() {
                m.store_word(control.active(), 0);
            }
            status
        }
    };
    unlock(m, context);
    status
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_stop_core")]
pub fn stop<M: Machine>(m: &M, entry: Domain, id: EncodedId) -> CyclicStatus {
    if m.handler_context() != 0 || m.irq_masked() {
        return CyclicStatus::Context;
    }
    let context = match target_context(m, entry, id.class()) {
        Ok(context) => context,
        Err(status) => return status,
    };
    let index = id.index();
    if index <= 0 {
        return CyclicStatus::InvalidIndex;
    }
    let count = m.load_word(context.cyclic_count());
    let slot = match CyclicSlot::new(index as u32, count) {
        Some(slot) => slot,
        None => return CyclicStatus::InvalidIndex,
    };
    let control = match control_of(m, context, slot) {
        Some(control) => control,
        None => return CyclicStatus::InvalidIndex,
    };
    let config = config_of(m, control);
    if m.load_word(config.exists()) == 0 {
        return CyclicStatus::NotCreated;
    }
    lock(m, context);
    let status = if m.remove_timer(context, control.timer()) {
        m.store_word(control.active(), 0);
        CyclicStatus::Ok
    } else {
        CyclicStatus::Timer
    };
    unlock(m, context);
    status
}

#[cfg_attr(feature = "stage", inline(never))]
#[cfg_attr(feature = "stage", link_section = ".text.cyclic_expire_core")]
pub fn expire<M: Machine>(m: &M, control: CyclicControl) -> CyclicStatus {
    let config = config_of(m, control);
    let period = PeriodWord(m.load_word(config.period()));
    let handler = CyclicHandler(m.load_word(config.handler()));
    let argument = CyclicArgument(m.load_word(config.argument()));
    let expiry = m.load_word(control.expiry());
    let context = m.current_context();
    let status = match period.checked() {
        None => {
            m.store_word(control.active(), 0);
            if m.remove_timer(context, control.timer()) {
                CyclicStatus::Ok
            } else {
                CyclicStatus::Timer
            }
        }
        Some(period) => {
            let expiry = Expiry(expiry.wrapping_add(period.get()));
            match schedule(m, context, control, expiry) {
                Ok(()) => CyclicStatus::Ok,
                Err(status) => status,
            }
        }
    };
    m.release_lock(context);
    m.irq_clear();
    m.invoke(handler, argument);
    m.irq_set();
    let context = m.current_context();
    lock(m, context);
    status
}
