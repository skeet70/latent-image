//! Machine 7, the playback request-routing machine. Its 49 actions (table indices 2..=50 of the
//! action table at 0x0121a490) read a status from retained request services, route on the
//! request event word at 0x014ea710, set the machine's state halfword and return the
//! dispatcher's result; where stock returns a service's r0, the action returns that result.

/// The state halfword of machine 7 (0x014ea5ba).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
    S5 = 5,
    S6 = 6,
    S7 = 7,
    S8 = 8,
    S9 = 9,
    S10 = 10,
    S11 = 11,
    S12 = 12,
    S13 = 13,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120d618
    Fn120d618,
    /// 0x0120e6e8
    Fn120e6e8,
    /// 0x0120e964
    Fn120e964,
    /// 0x0120ec28
    Fn120ec28,
    /// 0x0120f328
    Fn120f328,
    /// 0x0120f420
    Fn120f420,
    /// 0x0120fc08
    Fn120fc08,
    /// 0x0121164c
    Fn121164c,
    /// 0x01211c8c
    Fn1211c8c,
    /// 0x012128a0
    Fn12128a0,
    /// 0x01212a48
    Fn1212a48,
    /// 0x012138ac
    Fn12138ac,
    /// 0x0121406c
    Fn121406c,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x012390d4
    Fn12390d4,
    /// 0x01239134
    Fn1239134,
    /// 0x01239178
    Fn1239178,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x012290cc
    Fn12290cc,
    /// 0x012290fc, stores its argument at 0x014eb5d4
    Fn12290fc,
}

/// What machine 7 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    /// The word at 0x014ea710.
    fn request_event(&self) -> u32;
    /// The word at 0x014eb5d0.
    fn step(&self) -> u32;
    /// The halfword at 0x014eb5d4.
    fn step_kind(&self) -> u16;
    fn set_step_kind(&mut self, kind: u16);
}

const DONE: i32 = 8;
const REFUSED: i32 = 255;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn done_or_keep<S: Services>(services: &mut S, status: i32) -> i32 {
    if status == DONE {
        services.set_state(State::S0);
    }
    status
}

fn done_or<S: Services>(services: &mut S, status: i32, other: State) -> i32 {
    set(services, if status == DONE { State::S0 } else { other }, status)
}

fn pend<S: Services>(services: &mut S, status: i32, kind: u32) -> i32 {
    services.set_state(State::S10);
    services.call_with(CallWith::Fn12290cc, status as u32);
    services.call_with(CallWith::Fn12290fc, kind);
    0
}

fn settle<S: Services>(services: &mut S, call: Call, kind: u32) -> i32 {
    let status = services.call(call);
    if status == 0 {
        return 0;
    }
    if services.call(Call::Fn1239178) != 0 {
        return pend(services, status, kind);
    }
    services.call(Call::Fn12390d4);
    if status == DONE {
        return set(services, State::S0, DONE);
    }
    status
}

fn done_pending<S: Services>(services: &mut S, kind: u32) -> i32 {
    if services.call(Call::Fn1239178) == 0 {
        services.set_state(State::S0);
        services.call(Call::Fn12390d4);
        return DONE;
    }
    pend(services, DONE, kind)
}

fn gated_done<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn121164c);
    if status != DONE {
        return status;
    }
    done_pending(services, 1)
}

#[derive(Clone, Copy)]
enum Route {
    Fn120e964,
    Fn120f328,
    Fn120d618,
    Fn120ec28,
    Refuse,
}

fn route<S: Services>(services: &mut S, route: Route) -> i32 {
    let (call, kind, state) = match route {
        Route::Fn120e964 => (Call::Fn120e964, 2, State::S2),
        Route::Fn120f328 => (Call::Fn120f328, 4, State::S3),
        Route::Fn120d618 => (Call::Fn120d618, 7, State::S6),
        Route::Fn120ec28 => (Call::Fn120ec28, 5, State::S5),
        Route::Refuse => {
            services.call(Call::Trace);
            return REFUSED;
        }
    };
    let status = services.call(call);
    services.call_with(CallWith::Fn12290fc, kind);
    if status != DONE {
        return set(services, state, status);
    }
    if services.call(Call::Fn1239178) == 0 {
        services.set_state(State::S0);
        services.call(Call::Fn12390d4);
        return DONE;
    }
    services.set_state(State::S10);
    services.call_with(CallWith::Fn12290cc, DONE as u32);
    0
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn121164c);
    done_or_keep(services, status)
}

fn gated_keep<S: Services>(services: &mut S, call: Call) -> i32 {
    services.call(Call::Fn121164c);
    let status = services.call(call);
    done_or_keep(services, status)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    gated_keep(services, Call::Fn120e964)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    gated_keep(services, Call::Fn120f328)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    gated_keep(services, Call::Fn120fc08)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    let status = services.call(Call::Fn120ec28);
    if status == 0 {
        return 0;
    }
    if services.call(Call::Fn1239178) != 0 {
        return pend(services, status, 5);
    }
    services.call(Call::Fn12390d4);
    if status == 12 || status == DONE {
        services.set_state(State::S0);
    }
    status
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    gated_keep(services, Call::Fn120d618)
}

fn single<S: Services>(services: &mut S, call: Call, other: State) -> i32 {
    let status = services.call(call);
    done_or(services, status, other)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn12138ac, State::S7)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn1212a48, State::S8)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn12128a0, State::S9)
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    single(services, Call::Fn1211c8c, State::S12)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1239134);
    services.call(Call::Fn120e6e8);
    services.call(Call::Fn12390d4);
    DONE
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120f328);
    gated_done(services)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    settle(services, Call::Fn120e964, 2)
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    settle(services, Call::Fn120f328, 4)
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    settle(services, Call::Fn120fc08, 4)
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    settle(services, Call::Fn120d618, 7)
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn12290cc, DONE as u32);
    0
}

pub fn action21<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn120f420, State::S11)
}

pub fn action22<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn1211c8c, State::S12)
}

pub fn action23<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn121406c, State::S13)
}

pub fn action24<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120e964, 2)
}

pub fn action25<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120d618, 7)
}

pub fn action26<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn12290cc, 4);
    0
}

pub fn action27<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn121164c);
    set(services, if status == 12 { State::S0 } else { State::S1 }, status)
}

pub fn action28<S: Services>(services: &mut S) -> i32 {
    gated_done(services)
}

pub fn action29<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn121164c);
    if status == 0 {
        return set(services, State::S1, 0);
    }
    if services.call(Call::Fn1239178) != 0 {
        return pend(services, status, 1);
    }
    services.call(Call::Fn12390d4);
    if status == DONE {
        set(services, State::S0, DONE)
    } else {
        set(services, State::S1, status)
    }
}

pub fn action30<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120ec28)
}

pub fn action31<S: Services>(services: &mut S) -> i32 {
    gated_done(services)
}

pub fn action32<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    let selected = match services.request_event() {
        0 | 16 | 19 | 20 | 72 | 73 | 243 | 244 => Route::Fn120e964,
        1 | 2 | 4 | 6..=9 | 12 | 77 => Route::Fn120f328,
        5 | 29 => Route::Fn120d618,
        33..=36 | 40 | 140 | 143 | 144 => Route::Fn120ec28,
        _ => Route::Refuse,
    };
    route(services, selected)
}

pub fn action33<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    let selected = match services.request_event() {
        0 | 16 | 19 | 20 | 72 | 73 | 243 | 244 => Route::Fn120e964,
        1 | 2 | 4 | 6..=9 | 12 | 77 => Route::Fn120f328,
        5 | 29 => Route::Fn120d618,
        33..=36 | 40 | 140 | 144 => Route::Fn120ec28,
        _ => Route::Refuse,
    };
    route(services, selected)
}

pub fn action34<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    single(services, Call::Fn120ec28, State::S5)
}

pub fn action35<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    let selected = match services.request_event() {
        0 | 19 => Route::Fn120e964,
        5 => Route::Fn120d618,
        _ => Route::Refuse,
    };
    route(services, selected)
}

fn done_twelve_or<S: Services>(services: &mut S, status: i32, other: State) -> i32 {
    let state = match status {
        DONE => State::S0,
        12 => State::S2,
        _ => other,
    };
    set(services, state, status)
}

pub fn action36<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    let status = services.call(Call::Fn120ec28);
    done_twelve_or(services, status, State::S5)
}

pub fn action37<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn120ec28, State::S5)
}

pub fn action38<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120fc08);
    done_twelve_or(services, status, State::S4)
}

pub fn action39<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn120fc08, State::S4)
}

pub fn action40<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121164c);
    single(services, Call::Fn120f328, State::S3)
}

pub fn action41<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120e964);
    services.call_with(CallWith::Fn12290fc, 2);
    done_or(services, status, State::S2)
}

pub fn action42<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120f420);
    done_or_keep(services, status)
}

pub fn action43<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120e6e8);
    0
}

pub fn action44<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120e6e8);
    services.call(Call::Fn12390d4);
    if services.step() == DONE as u32 {
        services.set_state(State::S0);
        services.set_step_kind(0);
        return DONE;
    }
    let state = match services.step_kind() {
        1 => State::S1,
        2 => State::S2,
        4 => State::S3,
        7 => State::S6,
        _ => State::S0,
    };
    services.set_state(state);
    services.step() as i32
}

pub fn action45<S: Services>(services: &mut S) -> i32 {
    set(services, State::S2, 0)
}

pub fn action46<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn121164c);
    if status != DONE {
        return status;
    }
    if services.call(Call::Fn1239178) == 0 {
        services.set_state(State::S0);
        services.call(Call::Fn12390d4);
        return DONE;
    }
    services.set_state(State::S10);
    services.call_with(CallWith::Fn12290fc, 1);
    services.call_with(CallWith::Fn12290cc, DONE as u32);
    0
}

pub fn action47<S: Services>(services: &mut S) -> i32 {
    let result = settle(services, Call::Fn120e964, 2);
    if cfg!(playback_control_m7_action47_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action48<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120f328, 4)
}

pub fn action49<S: Services>(services: &mut S) -> i32 {
    single(services, Call::Fn120fc08, State::S4)
}

pub fn action50<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120ec28, 5)
}
