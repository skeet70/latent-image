"""One explicitly armed FFF1C initialization reset, without an engine job.

Whole10CDA6C clears its software owner, then93199C resets control/polls bit0
and93196C writes ACK/disable. This declared timed idle response supplies no
pixels, completion IRQ, saved callback or active-engine register law.
"""
from copy import deepcopy

from unicorn.arm_const import UC_ARM_REG_LR, UC_ARM_REG_PC

from .hardware import UnsupportedDevice
from .histogram_protocol import digest


PROFILE = 'declared-still-f1c-idle-reset-unmeasured'
BASE, ATTRIBUTE, IRQ = 0xfff1c000, 'f1c_idle', 50
MAX_POLLS = 200000
FIELDS = {'profile', 'state', 'armed_tick', 'idle_ticks', 'requested_tick',
          'due_tick', 'ready_tick', 'acknowledged_tick', 'disabled_tick',
          'polls', 'busy_polls', 'ready_polls', 'registers', 'state_sha256'}


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _seal(job):
    job['state_sha256'] = digest({k: v for k, v in job.items() if k != 'state_sha256'})


def _parent_idle(device):
    from .still_processing_devices import PROFILE as processing_profile
    if (device.profile != processing_profile or device.state != 'idle'
            or device.request is not None or device.board.failure is not None):
        raise UnsupportedDevice('F1C initialization requires its idle composite owner without a pixel job')


def _fresh_initialization(device):
    _parent_idle(device)
    if device.board.read_memory(0x173d9ec, 0x3c, cpu_id=0) != bytes(0x3c):
        raise UnsupportedDevice('F1C initialization requires the actual cleared10CDA6C software owner')
    if any(b.enabled & (1 << IRQ) or b.pending & (1 << IRQ)
           or b.active == IRQ or b.injected == IRQ for b in device.board.interrupts.banks):
        raise UnsupportedDevice('F1C initialization cannot coexist with a live result IRQ50 route')


def arm(device, profile=PROFILE, *, idle_ticks=16):
    if (profile != PROFILE or owner(device) is not None
            or type(idle_ticks) is not int or not 1 <= idle_ticks <= 400000):
        raise UnsupportedDevice('F1C initialization requires its explicit one-shot bounded idle profile')
    _fresh_initialization(device)
    now = device.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - idle_ticks:
        raise UnsupportedDevice('F1C initialization idle time overflows')
    job = dict(profile=PROFILE, state='awaiting-reset', armed_tick=now,
        idle_ticks=idle_ticks, requested_tick=None, due_tick=None, ready_tick=None,
        acknowledged_tick=None, disabled_tick=None, polls=0, busy_polls=0, ready_polls=0,
        registers={BASE: 0, BASE + 0xc8: 0, BASE + 0xcc: 0})
    _seal(job)
    setattr(device, ATTRIBUTE, job)
    device.phase_busy = True
    return summary(device)


def contains(device, address, size):
    return owner(device) is not None and BASE <= address < BASE + 0x1000


def validate(device):
    job = owner(device)
    if job is None:
        return None
    phases = ('awaiting-reset', 'reset-requested', 'idle-ready', 'acknowledged', 'complete')
    if (type(job) is not dict or set(job) != FIELDS or job['profile'] != PROFILE
            or job['state'] not in phases
            or job['state_sha256'] != digest({k: v for k, v in job.items() if k != 'state_sha256'})
            or type(job['armed_tick']) is not int or not 0 <= job['armed_tick'] <= device.board.virtual_ticks
            or type(job['idle_ticks']) is not int or not 1 <= job['idle_ticks'] <= 400000
            or any(type(job[k]) is not int or not 0 <= job[k] <= MAX_POLLS
                   for k in ('polls', 'busy_polls', 'ready_polls'))
            or job['polls'] != job['busy_polls'] + job['ready_polls']
            or type(job['registers']) is not dict or set(job['registers']) != {BASE, BASE + 0xc8, BASE + 0xcc}):
        raise UnsupportedDevice('F1C initialization changed its sealed idle lifecycle')
    phase = phases.index(job['state'])
    last = job['armed_tick']
    for required, key in ((phase >= 1, 'requested_tick'), (phase >= 2, 'ready_tick'),
                          (phase >= 3, 'acknowledged_tick'), (phase >= 4, 'disabled_tick')):
        value = job[key]
        if required:
            if type(value) is not int or not last <= value <= device.board.virtual_ticks:
                raise UnsupportedDevice('F1C idle request/response/ACK chronology changed')
            last = value
        elif value is not None:
            raise UnsupportedDevice('F1C initialization claims a premature physical phase')
    if (phase == 0 and (job['due_tick'] is not None or job['polls'])
            or phase >= 1 and job['due_tick'] != job['requested_tick'] + job['idle_ticks']
            or phase >= 2 and not job['due_tick'] <= job['ready_tick']
            or phase < 2 and job['ready_polls']
            or phase >= 3 and not job['ready_polls']
            or job['registers'] != {BASE: 1 if phase >= 2 else 0, BASE + 0xc8: 0, BASE + 0xcc: 0}
            or getattr(device, 'phase_busy', None) is not (phase < 4)):
        raise UnsupportedDevice('F1C initialization WORDs/busy lifecycle changed')
    return job


def access(device, operation, cpu, address, size, value=None):
    job = validate(device)
    if (job is None or operation not in ('read', 'write') or type(cpu) is not int or cpu != 0
            or type(size) is not int or size != 4 or address not in job['registers']):
        raise UnsupportedDevice('F1C idle law admits only its CPU0 control/ACK/disable WORDs')
    _fresh_initialization(device)
    if operation == 'read':
        if address != BASE or job['state'] not in ('reset-requested', 'idle-ready'):
            raise UnsupportedDevice('F1C idle polling precedes its actual reset request')
        if job['polls'] >= MAX_POLLS:
            raise UnsupportedDevice('F1C idle polling exceeds its finite contract')
        job['polls'] += 1
        job['ready_polls' if job['state'] == 'idle-ready' else 'busy_polls'] += 1
        _seal(job)
        return job['registers'][BASE]
    if type(value) is not int or not 0 <= value < 1 << 32:
        raise UnsupportedDevice('F1C idle write is not an unsigned WORD')
    now = device.board.virtual_ticks
    if address == BASE and value == 0 and job['state'] == 'awaiting-reset':
        core = device.board.cpus[0]
        if (int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != (0x9319ac, 0x10cda90):
            raise UnsupportedDevice('F1C reset requires the real10CDA6C initialization caller')
        if now >= (1 << 64) - job['idle_ticks']:
            raise UnsupportedDevice('F1C idle response time overflows')
        job.update(state='reset-requested', requested_tick=now, due_tick=now + job['idle_ticks'])
    elif address == BASE + 0xc8 and value == 1 and job['state'] == 'idle-ready' and job['ready_polls']:
        job.update(state='acknowledged', acknowledged_tick=now)
    elif address == BASE + 0xcc and value == 0 and job['state'] == 'acknowledged':
        job.update(state='complete', disabled_tick=now)
        device.phase_busy = False
    else:
        raise UnsupportedDevice('F1C active job or reordered idle ACK/disable remains unqualified')
    _seal(job)


def tick(device):
    job = owner(device)
    if job is None or job['state'] != 'reset-requested' or device.board.virtual_ticks < job['due_tick']:
        return False
    validate(device)
    _fresh_initialization(device)
    job['registers'][BASE] = 1
    job.update(state='idle-ready', ready_tick=device.board.virtual_ticks)
    _seal(job)
    return True


def next_event_tick(device, now):
    job = owner(device)
    return max(now, job['due_tick']) if job is not None and job['state'] == 'reset-requested' else None


def summary(device):
    return deepcopy(validate(device))
