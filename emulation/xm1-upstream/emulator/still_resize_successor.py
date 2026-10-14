"""One explicit named successor to a genuinely closed resize lifetime.

The stock 97ACD4 opening clears copied request RAM before its idle poll.
Archive the closed owner before that opening; never reset it implicitly.
Pixel planning and arithmetic stay in the existing StillResizeControl law.
"""
from copy import deepcopy
import hashlib
import json

from .hardware import UnsupportedDevice


PROFILE = 'declared-mode0-resize-second-lifetime-unmeasured'
REQUEST_KEYS = frozenset(('descriptor', 'plan', 'source_sha256', 'rows',
    'row_identity', 'completion_ticks', 'start_step', 'started_tick',
    'completed_tick', 'acknowledged_tick', 'idle_tick', 'closed_tick', 'seal_sha256'))


def _digest(record):
    request = record['request']
    plain = dict(request={k: v for k, v in request.items() if k not in ('rows', 'descriptor')},
        descriptor_sha256=hashlib.sha256(request['descriptor']).hexdigest(),
        registers=record['registers'], uploads=record['uploads'],
        ram_descriptor_sha256=hashlib.sha256(record['ram_descriptor']).hexdigest())
    return hashlib.sha256(json.dumps(plain, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def _closed(record):
    from .still_resize_devices import _seal, _request_plan
    from .still_resize_preview import PROFILE as PREVIEW_PROFILE
    from .still_resize_thumbnail import PROFILE as THUMBNAIL_PROFILE
    request = record['request']
    allowed_keys = REQUEST_KEYS | {'scene_profile'} if request.get('scene_profile') in (PREVIEW_PROFILE, THUMBNAIL_PROFILE) else REQUEST_KEYS
    if (set(request) != allowed_keys or _seal(request) != request['seal_sha256']
            or _digest(record) != record['sha256']
            or _request_plan(request) != request['plan']
            or tuple((a, hashlib.sha256(row).hexdigest()) for a, row in request['rows']) != request['row_identity']
            or record['ram_descriptor'] != request['descriptor'][:76] + bytes(4)
            or request['start_step'] != 3
            or any(type(request[k]) is not int for k in ('started_tick', 'completed_tick',
                'acknowledged_tick', 'idle_tick', 'closed_tick'))
            or not request['started_tick'] <= request['completed_tick'] <= request['acknowledged_tick']
                <= request['idle_tick'] <= request['closed_tick']
            or any(record['registers'][offset] != word for offset, word in request['plan']['expected'].items())
            or tuple(record['registers'][offset] for offset in (0, 0x144, 0x148)) != (0x10000, 0, 0)):
        raise UnsupportedDevice('Still resize prior lifetime lost its closed immutable owner')


def _drained(device):
    for bank in device.board.interrupts.banks:
        if (bank.enabled & (1 << 74) or bank.pending & (1 << 74)
                or bank.active == 74 or bank.injected == 74
                or any(entry.get('line') == 74 for entry in getattr(bank, 'active_stack', []))):
            raise UnsupportedDevice('Still resize successor requires retired and drained IRQ74')


def arm(device, profile):
    from .still_resize_devices import PROFILE as SCENE_PROFILE
    from .still_resize_preview import PROFILE as PREVIEW_PROFILE
    keys = REQUEST_KEYS | {'scene_profile'} if device.request is not None and device.request.get('scene_profile') == PREVIEW_PROFILE else REQUEST_KEYS
    if (profile != PROFILE or device.profile != SCENE_PROFILE or device.state not in ('closed', 'idle')
            or device.request is None or set(device.request) != keys
            or device.pending_action is not None or device.board.failure is not None):
        raise UnsupportedDevice('Still resize successor requires one explicit genuinely closed first lifetime')
    device._validate()
    _drained(device)
    if bytes(device.board.read_memory(0xc9ba3c, 4, cpu_id=0)) != bytes(4):
        raise UnsupportedDevice('Still resize successor precedes the stock unregister')
    archive = dict(request=deepcopy(device.request), registers=dict(device.registers),
        uploads=tuple(device.uploads), ram_descriptor=bytes(device.board.read_memory(0xc9ba40, 80, cpu_id=0)))
    archive['sha256'] = _digest(archive)
    _closed(archive)
    device.request['successor'] = dict(profile=PROFILE, state='awaiting-open',
        armed_tick=device.board.virtual_ticks, archive=archive)
    return summary(device)


def validate(device):
    marker = device.request.get('successor') if device.request is not None else None
    if marker is None:
        return False
    if (set(marker) != {'profile', 'state', 'armed_tick', 'archive'} or marker['profile'] != PROFILE
            or marker['state'] not in ('awaiting-open', 'opening', 'upload')
            or type(marker['armed_tick']) is not int or marker['armed_tick'] > device.board.virtual_ticks):
        raise UnsupportedDevice('Still resize successor marker changed')
    archive = marker['archive']
    _closed(archive)
    if ({k: v for k, v in device.request.items() if k != 'successor'} != archive['request']
            or tuple(device.uploads[:len(archive['uploads'])]) != archive['uploads']):
        raise UnsupportedDevice('Still resize successor changed its retained first lifetime')
    if marker['state'] == 'awaiting-open' and device.registers != archive['registers']:
        raise UnsupportedDevice('Still resize successor changed before the quiet opening')
    return True


def before_write(device, offset, value):
    """Own only the next whole quiet opening; ordinary upload stays typed."""
    marker = device.request.get('successor') if device.request is not None else None
    if marker is None:
        return False
    if marker['state'] == 'awaiting-open':
        validate(device)
        _drained(device)
        if ((offset, value) != (0, 0)
                or bytes(device.board.read_memory(0xc9ba38, 0x140, cpu_id=0)) != bytes(0x140)):
            raise UnsupportedDevice('Still resize successor requires whole97ACD4 clear and quiet opening')
        marker['state'] = 'opening'
        return False  # The existing idle law supplies this request/ACK.
    if marker['state'] == 'opening' and offset == 0x144:
        validate(device)
        if (value != 0x3f or device.state != 'idle' or device.pending_action is not None
                or device.registers[0] != 0x10000 or device.registers[0x144] != 0):
            raise UnsupportedDevice('Still resize successor status clear precedes the quiet ACK')
        device.registers[offset] = 0
        marker['state'] = 'upload'
        return True
    if marker['state'] == 'opening':
        raise UnsupportedDevice('Still resize successor requires the next quiet-opening status clear')
    return False


def setup_allowed(device):
    marker = device.request.get('successor') if device.request is not None else None
    return marker is not None and marker['state'] == 'upload'


def scene(device, *, profile, source_sha256, output_capacity, completion_ticks):
    from .still_resize_devices import PROFILE as SCENE_PROFILE
    from .still_resize_thumbnail import PROFILE as THUMBNAIL_PROFILE
    if (profile not in (SCENE_PROFILE, THUMBNAIL_PROFILE) or not setup_allowed(device) or device.state != 'idle'
            or device.pending_action is not None or device.board.failure is not None
            or type(completion_ticks) is not int or not 1 <= completion_ticks <= 400000):
        raise UnsupportedDevice('Still resize second scene requires its explicit completed opening/upload')
    validate(device)
    archive = device.request['successor']['archive']
    request = device._prepare_scene(source_sha256, output_capacity, completion_ticks, profile)
    request['history'] = [deepcopy(archive)]
    device.request, device.state = request, 'armed'
    return device.snapshot()


def history(device):
    records = device.request.get('history') if device.request is not None else None
    if records is None:
        return
    if type(records) is not list or len(records) != 1:
        raise UnsupportedDevice('Still resize supports exactly one retained predecessor')
    _closed(records[0])


def summary(device):
    validate(device)
    history(device)
    marker = device.request.get('successor') if device.request is not None else None
    record = marker['archive'] if marker else (device.request.get('history') or [None])[0] if device.request else None
    return dict(profile=PROFILE, state=marker['state'] if marker else 'scene' if record else 'unarmed',
        prior_sha256=record['sha256'] if record else None, lifetimes=2 if record else 1,
        limits='Exactly one explicit quiet reinitialization after closed mode0 or observed mode2 preview; named mode0 or observed mode1 thumbnail successor and actual IRQ74 closure.')
