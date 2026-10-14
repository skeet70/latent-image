"""One explicit second PFP initializer/job, preserving the complete first.

The observed source pointer advances3660 U16 samples. This declared scene
experiment shifts the candidate column by3660; it does not recover physical
calibration orientation or Fuji interpolation. A third lifetime is refused.
Temporary copyviews share the board only, never replace its guest state.
"""
from copy import copy, deepcopy

from . import prefilter_active as active, prefilter_initialization as idle
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-second-still-sparse-monochrome-prefilter-unmeasured'
ATTRIBUTE = 'prefilter_second'
SOURCE_ADVANCE = 7320
COUNT = 5371


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _first_anchor(device, *, full=False):
    first = active.validate(device, full=full)
    if first is None or first['state'] != 'complete':
        raise UnsupportedDevice('Second prefilter requires complete first pixels/IRQ/teardown')
    return dict(seal=first['seal'], contract_sha256=first['contract_sha256'], patches_sha256=first['patches_sha256'])


def _view(device, record=None):
    record = owner(device) if record is None else record
    view = copy(device)
    if hasattr(view, active.ATTRIBUTE): delattr(view, active.ATTRIBUTE)
    setattr(view, idle.ATTRIBUTE, record['initialization'])
    if record['job'] is not None: setattr(view, active.ATTRIBUTE, record['job'])
    return view


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k not in ('seal', 'initialization', 'job')})


def _new_request(device, first):
    request = active._words(device, 0x1552c20, 8)
    old = first['contract']['request']
    auxiliary_advance = ((old[5] * 34 + 63) & ~63) // 8
    expected = (old[0], old[1], old[2] + SOURCE_ADVANCE, old[3], old[4] + auxiliary_advance,
                COUNT, old[6], old[7])
    if request != expected:
        raise UnsupportedDevice('Second prefilter requires the actual bounded mode1 source/auxiliary continuation')
    return request


def arm_initialization(device, profile=PROFILE, *, idle_ticks=16):
    if profile != PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Second prefilter requires its explicit single successor profile')
    anchor = _first_anchor(device, full=True)
    previous = idle.summary(device)
    first = active.owner(device)
    request = _new_request(device, first)
    if (previous is None or previous['state'] != 'complete'
            or active._words(device, 0x152b724, 1) != (0,)
            or active._words(device, 0xbe391c, 1) != (1,)
            or active._words(device, 0xbe3920, 1) != (0,)):
        raise UnsupportedDevice('Second prefilter admission is the real fresh-open flags-zero boundary')
    geometry = first['contract']['geometry']
    if geometry['width'] * 2 <= 8 + SOURCE_ADVANCE:
        raise UnsupportedDevice('Second prefilter source leaves its owned RAW row')
    record = dict(profile=PROFILE, armed_tick=device.board.virtual_ticks, first_sha256=digest(anchor),
                  original_idle_sha256=digest(previous), request=request, request_sha256=digest(request),
                  initialization=None, job=None, seal=None)
    view = _view(device, record)
    # Old optional idle attr is removed only on this temporary host view.
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
            or record['first_sha256'] != digest(_first_anchor(device, full=full))
            or record['original_idle_sha256'] != digest(idle.summary(device))):
        raise UnsupportedDevice('Second prefilter changed its independent predecessor/continuation seals')
    view = _view(device)
    idle.validate(view)
    if record['job'] is not None: active.validate(view, full=full)
    return record


def arm_scene(device, profile=PROFILE, *, completion_ticks=1000, idle_ticks=16):
    record = validate(device, full=True)
    if profile != PROFILE or record is None or record['job'] is not None:
        raise UnsupportedDevice('Second prefilter scene requires its separately completed initializer')
    if active._words(device, 0x1552c20, 8) != record['request']:
        raise UnsupportedDevice('Second prefilter caller changed after initializer selection')
    view = _view(device)
    active._arm(view, active.PROFILE, completion_ticks=completion_ticks, idle_ticks=idle_ticks,
                source_offset=8 + SOURCE_ADVANCE, column_shift=SOURCE_ADVANCE // 2)
    if active.owner(view)['contract']['source_sha256'] != active.owner(device)['contract']['output_sha256']:
        raise UnsupportedDevice('Second prefilter must consume the exact first-job produced RAW image')
    record['job'] = active.owner(view)
    return summary(device)


def contains(device, address, size):
    return owner(device) is not None and idle.BASE <= address < idle.BASE + 0x230


def access(device, operation, core, address, size, value=None):
    record = validate(device)
    if record is None: raise UnsupportedDevice('Second prefilter is unselected')
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
    return result
