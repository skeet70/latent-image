"""The observed third mode3 PFP lifetime after two complete mode1 jobs.

Only the captured source+8/163-record request is admitted. This experiment
declares the same sparse four-neighbor kernel for mode3; that choice does not
identify physical mode3 interpolation. Predecessors remain immutable and a
fourth initializer requires a separate observed contract.
"""
from copy import copy, deepcopy

from . import prefilter_active as active, prefilter_initialization as idle, prefilter_second as second
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-third-mode3-still-sparse-monochrome-prefilter-unmeasured'
ATTRIBUTE = 'prefilter_third'
AUXILIARY, COUNT = 0x424289a8, 0xa3


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _predecessors(device, *, full=False):
    previous = second.validate(device, full=full)
    if previous is None or previous['job'] is None or previous['job']['state'] != 'complete':
        raise UnsupportedDevice('Third prefilter requires both complete mode1 jobs/IRQ closures')
    return dict(second_seal=previous['seal'], request_sha256=previous['request_sha256'],
                initialization_sha256=digest(previous['initialization']),
                job_seal=previous['job']['seal'], contract_sha256=previous['job']['contract_sha256'],
                patches_sha256=previous['job']['patches_sha256'])


def _view(device, record=None):
    record = owner(device) if record is None else record
    view = copy(device)
    if hasattr(view, active.ATTRIBUTE): delattr(view, active.ATTRIBUTE)
    setattr(view, idle.ATTRIBUTE, record['initialization'])
    if record['job'] is not None: setattr(view, active.ATTRIBUTE, record['job'])
    return view


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k not in ('seal', 'initialization', 'job')})


def _new_request(device):
    request = active._words(device, 0x1552c20, 8)
    first = active.owner(device)['contract']['request']
    expected = (3, first[1], first[2], first[3], AUXILIARY, COUNT, first[6], first[7])
    if request != expected:
        raise UnsupportedDevice('Third prefilter requires the observed bounded mode3 source/calibration request')
    return request


def arm_initialization(device, profile=PROFILE, *, idle_ticks=16):
    if profile != PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Third prefilter requires its explicit single mode3 successor')
    anchor = _predecessors(device, full=True)
    request = _new_request(device)
    if (active._words(device, 0x152b724, 1) != (0,)
            or active._words(device, 0xbe391c, 1) != (1,)
            or active._words(device, 0xbe3920, 1) != (0,)):
        raise UnsupportedDevice('Third prefilter admission is the actual fresh-open flags-zero boundary')
    record = dict(profile=PROFILE, armed_tick=device.board.virtual_ticks,
                  predecessor_sha256=digest(anchor), request=request, request_sha256=digest(request),
                  initialization=None, job=None, seal=None)
    view = _view(device, record)
    delattr(view, idle.ATTRIBUTE)
    idle.arm(view, idle.PROFILE, idle_ticks=idle_ticks)
    record['initialization'] = idle.owner(view)
    _seal(record); setattr(device, ATTRIBUTE, record)
    return summary(device)


def validate(device, *, full=False):
    record = owner(device)
    if record is None: return None
    if (type(record) is not dict or record.get('profile') != PROFILE
            or record.get('seal') != digest({k: v for k, v in record.items() if k not in ('seal', 'initialization', 'job')})
            or record['request_sha256'] != digest(record['request'])
            or record['predecessor_sha256'] != digest(_predecessors(device, full=full))):
        raise UnsupportedDevice('Third prefilter changed its independent predecessor/request seals')
    view = _view(device)
    idle.validate(view)
    if record['job'] is not None: active.validate(view, full=full)
    return record


def arm_scene(device, profile=PROFILE, *, completion_ticks=1000, idle_ticks=16):
    record = validate(device, full=True)
    if profile != PROFILE or record is None or record['job'] is not None:
        raise UnsupportedDevice('Third prefilter scene requires its separately completed initializer')
    if active._words(device, 0x1552c20, 8) != record['request']:
        raise UnsupportedDevice('Third prefilter caller changed after initializer selection')
    view = _view(device)
    active._arm(view, active.PROFILE, completion_ticks=completion_ticks, idle_ticks=idle_ticks, start_mode=3)
    if active.owner(view)['contract']['source_sha256'] != second.owner(device)['job']['contract']['output_sha256']:
        raise UnsupportedDevice('Third prefilter must consume the exact second-job produced RAW image')
    record['job'] = active.owner(view)
    return summary(device)


def contains(device, address, size):
    return owner(device) is not None and idle.BASE <= address < idle.BASE + 0x230


def access(device, operation, core, address, size, value=None):
    record = validate(device)
    if record is None: raise UnsupportedDevice('Third prefilter is unselected')
    view = _view(device)
    return (active.access(view, operation, core, address, size, value)
            if record['job'] is not None else idle.access(view, operation, core, address, size, value))


def tick(device):
    record = owner(device)
    if record is None: return False
    view = _view(device)
    pending = record['job']
    if (pending is not None and pending['state'] == 'running'
            and device.board.virtual_ticks >= pending['due_tick']):
        validate(device, full=True)
    return active.tick(view) if record['job'] is not None else idle.tick(view)


def next_event_tick(device, now):
    record = owner(device)
    if record is None: return None
    view = _view(device)
    return active.next_event_tick(view, now) if record['job'] is not None else idle.next_event_tick(view, now)


def summary(device):
    record = validate(device, full=True)
    if record is None: return None
    result = deepcopy({k: v for k, v in record.items() if k not in ('job', 'seal')})
    result['job'] = active.summary(_view(device)) if record['job'] is not None else None
    result['pixel_limits'] = 'Declared reuse of sparse mode1 kernel; mode3 physical interpolation unqualified'
    return result
