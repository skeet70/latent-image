"""One explicitly selected FFF1A idle initialization, without a pixel job.

StillProcessingControl is a composite container for this separate physical
engine. Its existing FFF400 registers and constructor state stay unchanged.
Only whole10D40C4's flags, idle poll and ACK are modeled; active processing,
source packing, auxiliary data, filtering and IRQ completion remain unknown.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-prefilter-idle-initialization-unmeasured'
BASE = 0xfff1a000
ATTRIBUTE = 'prefilter_initialization'
MAX_POLLS = 4096
FIELDS = {'profile', 'state', 'armed_tick', 'idle_ticks', 'flags_tick',
          'requested_tick', 'due_tick', 'ready_tick', 'acknowledged_tick',
          'polls', 'busy_polls', 'ready_polls', 'ready_poll_boundary',
          'registers', 'state_sha256'}


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _seal(job):
    job['state_sha256'] = digest({k: v for k, v in job.items() if k != 'state_sha256'})


def _parent_idle(device):
    from .still_processing_devices import PROFILE as processing_profile
    if (device.profile != processing_profile or device.state != 'idle'
            or device.request is not None or device.board.failure is not None):
        raise UnsupportedDevice('Prefilter initialization requires an idle composite owner, without a processing job')


def arm(device, profile=PROFILE, *, idle_ticks=16):
    if (profile != PROFILE or type(profile) is not str or owner(device) is not None
            or type(idle_ticks) is not int or not 1 <= idle_ticks <= 400000):
        raise UnsupportedDevice('Prefilter initialization requires its named one-shot bounded idle profile')
    _parent_idle(device)
    now = device.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - idle_ticks:
        raise UnsupportedDevice('Prefilter idle time exceeds unsigned virtual ticks')
    job = dict(profile=PROFILE, state='awaiting-flags', armed_tick=now,
        idle_ticks=idle_ticks, flags_tick=None, requested_tick=None, due_tick=None,
        ready_tick=None, acknowledged_tick=None, polls=0, busy_polls=0, ready_polls=0,
        ready_poll_boundary=None, registers={BASE: 0, BASE + 0x1c: 0, BASE + 0x20: 0})
    _seal(job)
    setattr(device, ATTRIBUTE, job)
    return summary(device)


def contains(device, address, size):
    # Claim the physical bank only after explicit selection, so unsupported
    # source/config/active writes produce a precise refusal, without output.
    return owner(device) is not None and BASE <= address < BASE + 0x230


def validate(device):
    job = owner(device)
    if job is None:
        return None
    phases = ('awaiting-flags', 'awaiting-idle-request', 'idle-requested', 'idle-ready', 'complete')
    if (type(job) is not dict or set(job) != FIELDS or job['profile'] != PROFILE
            or job['state'] not in phases
            or job['state_sha256'] != digest({k: v for k, v in job.items() if k != 'state_sha256'})
            or type(job['armed_tick']) is not int or not 0 <= job['armed_tick'] <= device.board.virtual_ticks
            or type(job['idle_ticks']) is not int or not 1 <= job['idle_ticks'] <= 400000
            or any(type(job[k]) is not int or not 0 <= job[k] <= MAX_POLLS
                   for k in ('polls', 'busy_polls', 'ready_polls'))
            or job['polls'] != job['busy_polls'] + job['ready_polls']
            or type(job['registers']) is not dict or set(job['registers']) != {BASE, BASE + 0x1c, BASE + 0x20}):
        raise UnsupportedDevice('Prefilter initialization changed its sealed finite idle lifecycle')
    phase = phases.index(job['state'])
    times = (job['flags_tick'], job['requested_tick'], job['ready_tick'], job['acknowledged_tick'])
    required = (phase >= 1, phase >= 2, phase >= 3, phase >= 4)
    last = job['armed_tick']
    for value, present in zip(times, required):
        if present:
            if type(value) is not int or not last <= value <= device.board.virtual_ticks:
                raise UnsupportedDevice('Prefilter idle request/response/ACK chronology changed')
            last = value
        elif value is not None:
            raise UnsupportedDevice('Prefilter initialization claims a premature physical phase')
    if (phase < 2 and (job['due_tick'] is not None or job['polls'])
            or phase >= 2 and job['due_tick'] != job['requested_tick'] + job['idle_ticks']
            or phase >= 3 and not job['due_tick'] <= job['ready_tick']
            or phase < 3 and (job['ready_poll_boundary'] is not None or job['ready_polls'])
            or phase >= 3 and (type(job['ready_poll_boundary']) is not int
                or job['ready_poll_boundary'] != job['busy_polls'])
            or phase == 4 and job['ready_polls'] == 0
            or job['registers'] != {BASE: 0x10000 if phase >= 3 else 0,
                                   BASE + 0x1c: 0, BASE + 0x20: 0}):
        raise UnsupportedDevice('Prefilter idle WORDs or sealed busy/ready polling boundary changed')
    return job


def access(device, operation, core, address, size, value=None):
    job = validate(device)
    if (job is None or operation not in ('read', 'write') or type(core) is not int
            or core != 0 or type(size) is not int or size != 4
            or address not in job['registers']):
        raise UnsupportedDevice('Prefilter initialization admits only its CPU0 flags/control/ACK WORDs')
    _parent_idle(device)
    if operation == 'read':
        if address != BASE or job['state'] not in ('idle-requested', 'idle-ready', 'complete'):
            raise UnsupportedDevice('Prefilter read precedes the recovered idle request')
        if job['state'] != 'complete':
            if job['polls'] >= MAX_POLLS:
                raise UnsupportedDevice('Prefilter idle polling exceeds its bounded initializer')
            job['polls'] += 1
            job['ready_polls' if job['state'] == 'idle-ready' else 'busy_polls'] += 1
            _seal(job)
        return job['registers'][address]
    if type(value) is not int or not 0 <= value < 1 << 32:
        raise UnsupportedDevice('Prefilter initialization requires an unsigned WORD')
    now = device.board.virtual_ticks
    if address == BASE + 0x20 and value == 0 and job['state'] == 'awaiting-flags':
        read = lambda a: int.from_bytes(device.board.read_memory(a, 4), 'little')
        if read(0xbe391c) != 1 or read(0xbe3920) != 0 or read(0x152b724) != 0:
            raise UnsupportedDevice('Prefilter flags require whole10D40C4 fresh open/idle caller')
        job.update(state='awaiting-idle-request', flags_tick=now)
    elif address == BASE and value == 0 and job['state'] == 'awaiting-idle-request':
        if now >= (1 << 64) - job['idle_ticks']:
            raise UnsupportedDevice('Prefilter idle response time overflows')
        job.update(state='idle-requested', requested_tick=now, due_tick=now + job['idle_ticks'])
    elif address == BASE + 0x1c and value == 0x1f and job['state'] == 'idle-ready' and job['ready_polls']:
        job.update(state='complete', acknowledged_tick=now)
    else:
        raise UnsupportedDevice('Prefilter active job or reordered initializer remains unqualified')
    _seal(job)


def tick(device):
    job = owner(device)
    if job is None or job['state'] != 'idle-requested' or device.board.virtual_ticks < job['due_tick']:
        return False
    validate(device)
    _parent_idle(device)
    job['registers'][BASE] = 0x10000
    job.update(state='idle-ready', ready_tick=device.board.virtual_ticks,
               ready_poll_boundary=job['polls'])
    _seal(job)
    return True


def next_event_tick(device, now):
    job = owner(device)
    return max(now, job['due_tick']) if job is not None and job['state'] == 'idle-requested' else None


def summary(device):
    return deepcopy(validate(device))
