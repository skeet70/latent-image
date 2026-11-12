use crate::fake::{Call, Fake};
use crate::*;

fn task(number: u8) -> TaskId { TaskId::new(number).unwrap() }
fn semaphore(number: u8) -> SemaphoreId { SemaphoreId::new(number).unwrap() }

#[test]
fn status_round_trips_for_every_listed_word() {
    for word in [0, -17, -18, -25, -28, -41, -42, -43, -49, -50] {
        match Status(word).check() {
            Ok(()) => assert_eq!(word, 0),
            Err(error) => assert_eq!(error.status(), Status(word)),
        }
    }
}

#[test]
fn unlisted_status_is_kept_and_is_not_success() {
    for word in [1, -1, 7, -100, i32::MAX, i32::MIN] {
        assert_eq!(Status(word).check(), Err(Error::Other(Status(word))));
    }
}

#[test]
fn task_is_none_outside_one() {
    let kernel = Kernel::new(Fake::new());
    assert!(kernel.task().is_none());
}

#[test]
fn task_reports_the_backend_number() {
    let kernel = Kernel::new(Fake::with_task(task(3)));
    assert_eq!(kernel.task().unwrap().id(), task(3));
}

#[test]
fn context_token_follows_the_handler_word_then_the_task() {
    match Kernel::new(Fake::with_task(task(2))).context() {
        Context::Task(token) => assert_eq!(token.id(), task(2)),
        _ => panic!("task context expected"),
    }
    match Kernel::new(Fake::new().in_handler()).context() {
        Context::Interrupt(_) => {}
        _ => panic!("interrupt context expected"),
    }
    match Kernel::new(Fake::new()).context() {
        Context::Masked(_) => {}
        _ => panic!("masked context expected"),
    }
}

#[test]
fn signal_uses_the_task_form_from_a_task_token() {
    let kernel = Kernel::new(Fake::with_task(task(1)));
    let token = kernel.task().unwrap();
    assert_eq!(Semaphore::stock(semaphore(0x25)).signal(&token), Ok(()));
    assert_eq!(kernel.backend().calls(), [Call::CurrentTask, Call::Signal { id: semaphore(0x25), form: Form::Task }]);
}

#[test]
fn signal_uses_the_handler_form_from_an_interrupt_token() {
    let kernel = Kernel::new(Fake::new().in_handler());
    let Context::Interrupt(token) = kernel.context() else { panic!("interrupt context expected") };
    assert_eq!(Semaphore::stock(semaphore(0x25)).signal(&token), Ok(()));
    assert_eq!(kernel.backend().calls(), [Call::InHandler, Call::Signal { id: semaphore(0x25), form: Form::Handler }]);
}

#[test]
fn wait_encodes_each_timeout() {
    let kernel = Kernel::new(Fake::with_task(task(1)));
    let token = kernel.task().unwrap();
    let units = ClockUnits::new(7).unwrap();
    for (timeout, word) in [(Timeout::Poll, 0), (Timeout::Forever, u32::MAX), (Timeout::Units(units), 7)] {
        assert_eq!(Semaphore::stock(semaphore(1)).wait(&token, timeout), Ok(()));
        assert_eq!(kernel.backend().calls().last(), Some(&Call::Wait { id: semaphore(1), timeout: word }));
    }
}

#[test]
fn clock_units_refuse_zero_and_overflow() {
    assert_eq!(ClockUnits::new(0), None);
    assert_eq!(ClockUnits::new(i32::MAX as u32 + 1), None);
    assert_eq!(ClockUnits::from_ticks(0), None);
    assert_eq!(ClockUnits::from_ticks(1), ClockUnits::new(ClockUnits::PER_TICK));
    assert_eq!(ClockUnits::from_ticks(u32::MAX), None);
}

#[test]
fn wait_returns_the_backend_error() {
    let kernel = Kernel::new(Fake::with_task(task(1)).wait_result(Err(Error::Other(Status(-7)))));
    let token = kernel.task().unwrap();
    assert_eq!(Semaphore::stock(semaphore(1)).wait(&token, Timeout::Forever), Err(Error::Other(Status(-7))));
}

#[test]
fn irq_mask_restores_the_prior_state() {
    let kernel = Kernel::new(Fake::with_task(task(1)).irq_state(1));
    {
        let mut token = kernel.task().unwrap();
        let mask = token.mask_irq();
        assert_eq!(kernel.backend().calls(), [Call::CurrentTask, Call::IrqMask]);
        drop(mask);
    }
    assert_eq!(kernel.backend().calls(), [Call::CurrentTask, Call::IrqMask, Call::IrqRestore(1)]);
}

#[test]
fn a_mask_nested_in_a_mask_restores_both() {
    let kernel = Kernel::new(Fake::with_task(task(1)).irq_state(1));
    let mut token = kernel.task().unwrap();
    let mut outer = token.mask_irq();
    let inner = outer.lock_cores();
    drop(inner);
    drop(outer);
    assert_eq!(
        kernel.backend().calls(),
        [Call::CurrentTask, Call::IrqMask, Call::CoresLock, Call::CoresUnlock(0), Call::IrqRestore(1)]
    );
}

#[test]
fn lock_cores_calls_the_stock_pair_in_each_stamp_state() {
    for saved in [0u32, 0x11, 0x21] {
        let kernel = Kernel::new(Fake::with_task(task(1)).cores_state(saved));
        {
            let mut token = kernel.task().unwrap();
            let lock = token.lock_cores();
            assert_eq!(kernel.backend().calls(), [Call::CurrentTask, Call::CoresLock]);
            drop(lock);
        }
        assert_eq!(kernel.backend().calls(), [Call::CurrentTask, Call::CoresLock, Call::CoresUnlock(saved)]);
    }
}

#[test]
fn every_token_can_mask_and_lock() {
    let kernel = Kernel::new(Fake::with_task(task(1)));
    match kernel.context() {
        Context::Task(mut token) => { drop(token.mask_irq()); drop(token.lock_cores()); }
        _ => panic!("task context expected"),
    }
    let kernel = Kernel::new(Fake::new().in_handler());
    match kernel.context() {
        Context::Interrupt(mut token) => { drop(token.mask_irq()); drop(token.lock_cores()); }
        _ => panic!("interrupt context expected"),
    }
    let kernel = Kernel::new(Fake::new());
    match kernel.context() {
        Context::Masked(mut token) => { drop(token.mask_irq()); drop(token.lock_cores()); }
        _ => panic!("masked context expected"),
    }
}
