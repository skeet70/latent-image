"""The observed fourth, zero-count PFP configuration and idle teardown.

Stock10D4144 calls11806A8 synchronously when the auxiliary count is zero.
This owner produces no pixels, status, hardware start or interrupt. It retains
all three completed predecessors and refuses another lifecycle.
"""
from copy import copy, deepcopy
import hashlib

from . import prefilter_active as active, prefilter_initialization as idle, prefilter_third as third
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-fourth-empty-still-prefilter-control-unmeasured'
ATTRIBUTE = 'prefilter_empty'


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _predecessor(device, *, full=False):
    previous = third.validate(device, full=full)
    if previous is None or previous['job'] is None or previous['job']['state'] != 'complete':
        raise UnsupportedDevice('Empty prefilter requires all three closed pixels/IRQ histories')
    return dict(seal=previous['seal'], request_sha256=previous['request_sha256'],
                initialization_sha256=digest(previous['initialization']),
                job_seal=previous['job']['seal'], contract_sha256=previous['job']['contract_sha256'],
                patches_sha256=previous['job']['patches_sha256'])


def _view(device, record=None):
    record = owner(device) if record is None else record
    view = copy(device)
    setattr(view, idle.ATTRIBUTE, record['initialization'])
    return view


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k not in ('seal', 'initialization', 'configuration')})


def _new_request(device):
    request = active._words(device, 0x1552c20, 8)
    previous = third.owner(device)['request']
    advance = ((previous[5] * 34 + 63) & ~63) // 8
    expected = (previous[0], previous[1], previous[2] + 7320, previous[3], previous[4] + advance, 0, previous[6], previous[7])
    if request != expected:
        raise UnsupportedDevice('Empty prefilter requires the observed fourth mode3 zero-count segment')
    return request


def arm_initialization(device, profile=PROFILE, *, idle_ticks=16):
    if profile != PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Empty prefilter requires its explicit single zero-count profile')
    predecessor = _predecessor(device, full=True)
    request = _new_request(device)
    if (active._words(device, 0x152b724, 1) != (0,)
            or active._words(device, 0xbe391c, 1) != (1,)
            or active._words(device, 0xbe3920, 1) != (0,)):
        raise UnsupportedDevice('Empty prefilter admission precedes the actual fresh initializer')
    record = dict(profile=PROFILE, armed_tick=device.board.virtual_ticks,
                  predecessor_sha256=digest(predecessor), request=request, request_sha256=digest(request),
                  initialization=None, configuration=None, seal=None)
    view = _view(device, record); delattr(view, idle.ATTRIBUTE)
    idle.arm(view, idle.PROFILE, idle_ticks=idle_ticks)
    record['initialization'] = idle.owner(view)
    _seal(record); setattr(device, ATTRIBUTE, record)
    return summary(device)


def _config_seal(configuration):
    configuration['seal'] = digest({k: v for k, v in configuration.items() if k not in ('seal', 'contract')})


def _contract_hash(contract):
    return digest({k: v for k, v in contract.items() if k != 'qtable'})


def validate(device, *, full=False):
    record = owner(device)
    if record is None: return None
    if (type(record) is not dict or record.get('profile') != PROFILE
            or record.get('seal') != digest({k: v for k, v in record.items() if k not in ('seal', 'initialization', 'configuration')})
            or record['request_sha256'] != digest(record['request'])
            or record['predecessor_sha256'] != digest(_predecessor(device, full=full))):
        raise UnsupportedDevice('Empty prefilter changed its independent predecessor/request seals')
    idle.validate(_view(device))
    c = record['configuration']
    if c is not None:
        if (type(c) is not dict or c.get('state') not in ('configuring', 'configured', 'idle-requested', 'idle-ready', 'complete')
                or c.get('seal') != digest({k: v for k, v in c.items() if k not in ('seal', 'contract')})
                or c['contract_sha256'] != _contract_hash(c['contract'])
                or c['contract']['qtable_sha256'] != hashlib.sha256(c['contract']['qtable']).hexdigest()
                or c['contract']['request'] != record['request'] or c['contract']['request'][5] != 0
                or type(c['step']) is not int or not 0 <= c['step'] <= 134
                or (c['state'] == 'configuring') != (c['step'] < 134)
                or any(type(c[k]) is not int or not 0 <= c[k] <= active.MAX_POLLS for k in ('polls', 'busy_polls', 'ready_polls'))
                or c['polls'] != c['busy_polls'] + c['ready_polls']):
            raise UnsupportedDevice('Empty prefilter changed its bounded configuration/idle lifecycle')
        if c['idle_due_tick'] is not None and c['idle_due_tick'] != c['idle_requested_tick'] + c['idle_ticks']:
            raise UnsupportedDevice('Empty prefilter idle deadline changed')
        if c['idle_ready_tick'] is not None and c['idle_poll_boundary'] != c['busy_polls']:
            raise UnsupportedDevice('Empty prefilter idle response boundary changed')
    return record


def arm_scene(device, profile=PROFILE, *, idle_ticks=16):
    record = validate(device, full=True)
    if (profile != PROFILE or record is None or record['configuration'] is not None
            or type(idle_ticks) is not int or not 1 <= idle_ticks <= 400000
            or idle.owner(_view(device))['state'] != 'complete'
            or active._words(device, 0x152b724, 1) != (1,)):
        raise UnsupportedDevice('Empty prefilter configuration requires its separately completed initializer')
    active._route(device, armed=False)
    request = record['request']
    descriptor = (request[1], request[2], request[3], request[4], request[5], request[7], request[6])
    if (active._words(device, 0x1552c20, 8) != request or active._words(device, 0x1564fe4, 7) != descriptor):
        raise UnsupportedDevice('Empty prefilter zero-count caller changed before configuration')
    qtable = bytes(device.board.read_memory(request[7], 512, cpu_id=0))
    previous = third.owner(device)['job']['contract']
    geometry = previous['geometry']
    source_sha256 = hashlib.sha256(device.board.read_memory(geometry['destination'], geometry['span_bytes'], cpu_id=0)).hexdigest()
    if qtable != previous['qtable'] or source_sha256 != previous['output_sha256']:
        raise UnsupportedDevice('Empty prefilter changed its third output/coefficient ownership')
    contract = dict(request=request, descriptor=descriptor, qtable=qtable,
                    qtable_sha256=hashlib.sha256(qtable).hexdigest(),
                    source_sha256=source_sha256, geometry=deepcopy(geometry))
    c = dict(state='configuring', contract=contract, contract_sha256=_contract_hash(contract),
             armed_tick=device.board.virtual_ticks, step=0, idle_ticks=idle_ticks,
             idle_requested_tick=None, idle_due_tick=None, idle_ready_tick=None, idle_poll_boundary=None,
             callback_verified_tick=None, closed_tick=None, polls=0, busy_polls=0, ready_polls=0,
             registers={active.BASE + o: 0 for o in (0, 4, 8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x20)}, seal=None)
    c['registers'][active.BASE] = 0x10000
    _config_seal(c); record['configuration'] = c
    return summary(device)


def contains(device, address, size):
    return owner(device) is not None and idle.BASE <= address < idle.BASE + 0x230


def access(device, operation, core, address, size, value=None):
    record = validate(device)
    if record is None: raise UnsupportedDevice('Empty prefilter is unselected')
    c = record['configuration']
    if c is None: return idle.access(_view(device), operation, core, address, size, value)
    if (operation not in ('read', 'write') or type(core) is not int or core != 0 or size != 4
            or type(address) is not int or address & 3
            or operation == 'write' and (type(value) is not int or not 0 <= value < 1 << 32)):
        raise UnsupportedDevice('Empty prefilter admits only its CPU0 WORD transaction')
    now = device.board.virtual_ticks
    if c['state'] == 'configuring':
        expected = active._setup(c['contract'])[c['step']]
        if (operation, address, value) != expected:
            raise UnsupportedDevice('Empty prefilter requires the next exact source/Q/aux0/parameter store')
        if active._words(device, 0x1564fe4, 7) != c['contract']['descriptor']:
            raise UnsupportedDevice('Empty prefilter descriptor changed during setup')
        c['registers'][address] = value; c['step'] += 1
        if c['step'] == 134: c['state'] = 'configured'
    elif operation == 'write' and address == active.BASE and value == 0 and c['state'] == 'configured':
        #10D4214 skips IRQ flag disable/unregister when count is zero. It
        # clears its callback and proceeds directly to93C6EC idle control.
        active._route(device, armed=False)
        if (active._words(device, 0x152b724, 1) != (3,)
                or active._words(device, 0x152b748, 1) != (0,)
                or active._words(device, 0x152b73c, 1) != (0,)
                or active._words(device, 0xbe3920, 1) != (0,)):
            raise UnsupportedDevice('Empty prefilter close precedes the real zero-count callback branch')
        c['registers'][address] = 0
        c.update(state='idle-requested', callback_verified_tick=now,
                 idle_requested_tick=now, idle_due_tick=now + c['idle_ticks'])
    elif operation == 'read' and address == active.BASE and c['state'] in ('idle-requested', 'idle-ready'):
        if c['polls'] >= active.MAX_POLLS: raise UnsupportedDevice('Empty prefilter idle polling exceeds its physical bound')
        c['polls'] += 1; c['ready_polls' if c['state'] == 'idle-ready' else 'busy_polls'] += 1
        _config_seal(c); return c['registers'][address]
    elif operation == 'write' and address == active.BASE + 0x1c and value == 0x1f and c['state'] == 'idle-ready' and c['ready_polls']:
        c.update(state='complete', closed_tick=now)
    else:
        raise UnsupportedDevice('Empty prefilter has no hardware start/result/IRQ or reordered teardown')
    _config_seal(c)


def tick(device):
    record = owner(device)
    if record is None: return False
    c = record['configuration']
    if c is None: return idle.tick(_view(device))
    if c['state'] == 'idle-requested' and device.board.virtual_ticks >= c['idle_due_tick']:
        validate(device)
        c['registers'][active.BASE] = 0x10000
        c.update(state='idle-ready', idle_ready_tick=device.board.virtual_ticks, idle_poll_boundary=c['polls'])
        _config_seal(c); return True
    return False


def next_event_tick(device, now):
    record = owner(device)
    if record is None: return None
    c = record['configuration']
    return idle.next_event_tick(_view(device), now) if c is None else c['idle_due_tick'] if c['state'] == 'idle-requested' else None


def summary(device):
    record = validate(device, full=True)
    return deepcopy({k: v for k, v in record.items() if k != 'seal'}) if record is not None else None
