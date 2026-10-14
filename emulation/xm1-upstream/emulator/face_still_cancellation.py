"""One timer-owned cancellation of the later still Face desired job.

The named profile retains only the existing idle-DMA stop transaction.
The stock application timer, mailbox and callback own cancellation policy.
No face result, interrupt, callback, timer or firmware state is supplied.
"""
from copy import copy, deepcopy

from .hardware import UnsupportedDevice
from . import face_still_reconfiguration as previous

PROFILE = 'declared-still-face-deadline-idle-cancellation-unmeasured'
KEY = 'cancellation'
WORDS = ((0x15e32d0, 12), (0x15d502c, 11), (0x15d50a4, 1),
         (0x15d50a8, 0), (0x15d50bc, 0), (0x15d50c0, 1),
         (0x15d4f70, 0x125adc8), (0x41986db0, 2), (0x41986b28, 0),
         (0x4197f02c, 3), (0x4197f06c, 0x130f7ac))


def owner(device):
    job = previous.owner(device)
    return job.get(KEY) if type(job) is dict else None


def _base(device):
    return {k: v for k, v in previous.owner(device).items() if k != KEY}


def historical(device):
    job = owner(device)
    view = copy(device)
    view.stop = dict(device.stop, reconfiguration=_base(device))
    view.registers = job['initial_registers'].copy()
    view.dma_states = list(job['initial_dma_states'])
    return view


def _stop_view(device):
    view = copy(device)
    view.request = previous.owner(device)['request']
    view.stop = owner(device)['stop']
    return view


def _timer(device):
    b = device.board
    u = lambda a: int.from_bytes(b.read_memory(a, 4), 'little')
    handle = u(0x15d50b8)
    slots = [i for i in range(32) if u(0xc64ef0 + 4*i) == handle]
    if not handle or len(slots) != 1:
        raise UnsupportedDevice('Face cancellation requires its unique registered still deadline')
    slot = slots[0]
    words = [u(0xc64f74 + 20*slot + 4*i) for i in range(5)]
    banks = getattr(getattr(b, 'interrupts', None), 'banks', [])
    if len(banks) != 2:
        raise UnsupportedDevice('Face cancellation requires the architectural timer route')
    bank = banks[0]
    if (u(0xc64f70) != 1 or words[0] != 1 or not 1 <= words[1] <= 301 or
            words[2:] != [300, 1, 0x130f710] or not bank.enabled & (1 << 0x22) or
            bank.config.get(0x400 + 4*0x22) != 0x91d360 or
            bank.config.get(0x800 + 4*0x22) != 12 or not bank.source_config & (1 << 0x22) or
            banks[1].enabled & (1 << 0x22)):
        raise UnsupportedDevice('Face cancellation requires the actual CFG_A01B timer and 130F710 callback')
    return dict(handle=handle, slot=slot, reload=300, periodic=1, callback=0x130f710,
                route=dict(cpu=0, irq=0x22, vector=0x91d360, priority=12, source_type=1))


def _seal(device, job):
    return device._digest({k: v for k, v in job.items() if k != 'state_sha256'})


def validate(device):
    job = owner(device)
    fields = {'profile', 'cpu', 'state', 'initial_registers', 'initial_dma_states',
              'predecessor_sha256', 'contract', 'stop', 'requested_tick',
              'started_tick', 'completed_tick', 'state_sha256'}
    if (type(job) is not dict or set(job) != fields or job['profile'] != PROFILE or
            type(job['cpu']) is not int or job['cpu'] != 0 or
            job['state'] not in ('waiting-reset', 'stopping', 'complete', 'closed') or
            job['state_sha256'] != _seal(device, job) or
            job['predecessor_sha256'] != device._digest(_base(device)) or
            job['initial_dma_states'] != ['armed-awaiting-result']*4 or
            type(job['initial_registers']) is not dict):
        raise UnsupportedDevice('Still Face cancellation changed its retained desired-job owner')
    previous.validate(historical(device))
    c = job['contract']; t = c.get('timer') if type(c) is dict else None
    if (type(c) is not dict or set(c) != {'words', 'input_pointer', 'timer'} or
            c['words'] != [list(w) for w in WORDS] or
            c['input_pointer'] != previous.owner(device)['request']['input_pointer'] or
            type(t) is not dict or set(t) != {'handle', 'slot', 'reload', 'periodic', 'callback', 'route'} or
            type(t['handle']) is not int or not 0 < t['handle'] < 1 << 32 or
            type(t['slot']) is not int or not 0 <= t['slot'] < 32 or
            (t['reload'], t['periodic'], t['callback']) != (300, 1, 0x130f710) or
            t['route'] != dict(cpu=0, irq=0x22, vector=0x91d360, priority=12, source_type=1)):
        raise UnsupportedDevice('Still Face cancellation changed its captured timer/caller contract')
    if (type(job['requested_tick']) is not int or not 0 <= job['requested_tick'] <= device.board.virtual_ticks or
            any(v is not None and (type(v) is not int or not job['requested_tick'] <= v <= device.board.virtual_ticks)
                for v in (job['started_tick'], job['completed_tick'])) or
            job['completed_tick'] is not None and
            (job['started_tick'] is None or not job['started_tick'] <= job['completed_tick'])):
        raise UnsupportedDevice('Still Face cancellation changed its observed timing')
    view = _stop_view(device)
    if job['stop'] is None:
        if (job['state'] != 'waiting-reset' or job['started_tick'] is not None or
                job['completed_tick'] is not None or device.registers != job['initial_registers'] or
                device.dma_states != job['initial_dma_states']):
            raise UnsupportedDevice('Still Face cancellation claims work before its real reset')
    else:
        stop = job['stop']; step = stop.get('step') if type(stop) is dict else None
        if (not view._stop_valid() or stop['cpu'] != 0 or
                stop['initial_registers'] != job['initial_registers'] or
                job['started_tick'] is None or
                job['state'] != ('stopping' if step < 26 else 'complete' if step < 36 else 'closed') or
                (step >= 26) != (job['completed_tick'] is not None)):
            raise UnsupportedDevice('Still Face cancellation changed its exact idle-stop transaction')
    return job


def arm(device, profile):
    job = previous.validate(device)
    b = device.board
    u = lambda a: int.from_bytes(b.read_memory(a, 4), 'little')
    if (profile != PROFILE or owner(device) is not None or b.failure is not None or
            job['step'] != 126 or type(job['request']) is not dict or
            job['request']['phase'] != 'armed-awaiting-result' or
            device.stop_profile != device.STOP_PROFILE or
            any(u(a) != v for a, v in WORDS) or
            job['request']['input_pointer'] != hex(u(0x4197f048)) or
            any(device.registers[0x810 + 0x80*i] != 0 for i in range(4))):
        raise UnsupportedDevice('Still Face cancellation awaits the real private11 timer-owned desired job')
    contract = dict(words=[list(w) for w in WORDS], input_pointer=job['request']['input_pointer'],
                    timer=_timer(device))
    record = dict(profile=profile, cpu=0, state='waiting-reset',
                  initial_registers=deepcopy(device.registers), initial_dma_states=list(device.dma_states),
                  predecessor_sha256=device._digest(job), contract=contract, stop=None,
                  requested_tick=b.virtual_ticks, started_tick=None, completed_tick=None)
    record['state_sha256'] = _seal(device, record)
    job[KEY] = record
    return deepcopy(validate(device))


def access(device, operation, core, address, size, value=None):
    job = validate(device)
    view = _stop_view(device)
    result = view._stop_access(operation, core, address, size, value)
    job['stop'] = view.stop
    device.dma_states = view.dma_states
    step = view.stop['step']
    if job['started_tick'] is None:
        job['started_tick'] = device.board.virtual_ticks
    if step == 26:
        job['completed_tick'] = device.board.virtual_ticks
    job['state'] = 'stopping' if step < 26 else 'complete' if step < 36 else 'closed'
    job['state_sha256'] = _seal(device, job)
    return result


def summary(device):
    return deepcopy(validate(device)) if owner(device) is not None else None
