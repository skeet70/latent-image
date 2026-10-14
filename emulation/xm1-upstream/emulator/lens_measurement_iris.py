"""One declared iris-index11 movement requested by the stock AF slot table.

4213 -> A086F0 -> 4501 -> A17A8C maps index11 to configuration coordinate56
and emits quantity1792 (56 *32). This helper supplies only the physical IRQ12
after that complete transaction. A16F80/4509/A179D4 own position and callbacks.
It leaves the existing homing fixture archive unchanged and stores its ledger
in LensEndpoint.requests['iris'], an existing portable state field.
"""
from copy import deepcopy
import hashlib
import json

from .hardware import UnsupportedDevice

PROFILE = 'declared-afs-iris-index11-coordinate56-quantity1792-unmeasured'
WORDS = tuple(int(w, 16) for w in (
    '7C80 4214 5440 4E71 584C 5C4C 604C 644C 5280 78D0 '
    '6800 6C00 4700 4801 7EC0').split())
BODY = (0x12f86cc, 11, 1, 0, 1, 1)


def _note(endpoint, kind, **data):
    from .lens_focus_motion import lens_note
    lens_note(endpoint, kind, **data)


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def _read(board, address, size=4):
    return int.from_bytes(board.read_memory(address, size), 'little')


def _fixture(endpoint):
    from .lens_homing_fixture import LensHomingFixture
    f = endpoint.homing_fixture
    if (type(f) is not LensHomingFixture or f.board is not endpoint.board
            or f not in endpoint.board.devices):
        raise UnsupportedDevice('AF iris movement requires its attached complete lens archive')
    return f


def _route(endpoint, owner):
    banks = getattr(getattr(endpoint.board, 'interrupts', None), 'banks', ())
    if (len(banks) != 2 or owner != 0
            or not banks[0].enabled & (1 << 12) or banks[1].enabled & (1 << 12)
            or banks[0].config.get(0x430) != 0xa16f80
            or banks[0].config.get(0x830) != 12
            or (banks[0].source_config >> 12) & 1 != 1):
        raise UnsupportedDevice('AF iris response requires the stock CPU0 IRQ12 vector/priority/trigger')


def arm(endpoint, profile, *, _from_wire=False):
    """Arm before the whole iris driver runs, after real4213 has been latched."""
    from .lens_endpoint import HOMING_PROFILE
    b, f = endpoint.board, _fixture(endpoint)
    from .lens_af_motion import PROFILE as MOTION_PROFILE
    motion = endpoint.requests['focus']
    selected = (type(motion) is dict and motion == dict(profile=MOTION_PROFILE,
        state='waiting-for-iris', enabled_tick=motion.get('enabled_tick'))
        and type(motion.get('enabled_tick')) is int and 0 <= motion['enabled_tick'] <= b.virtual_ticks)
    if _from_wire and not selected:
        raise UnsupportedDevice('AF iris automatic wire admission requires its explicitly selected waiting profile')
    if (profile != PROFILE or endpoint.response_profile != HOMING_PROFILE
            or b.failure is not None or b.scheduled or endpoint.requests['iris'] is not None
            or (endpoint.requests['focus'] is not None and not selected)
            or any(p is not None for p in f.pending.values())
            or f.post_iris.get('state') != 'complete'
            or any(x is not None for x in (f.controller_reinit, f.controller_close, f.focus_reentry,
                                           getattr(f, 'iris_reentry', None)))
            or endpoint.words != f.words or endpoint.registers != f.registers
            or not endpoint.words or endpoint.words[-1]['word'] != 0x7c80
            or endpoint.registers.get(0x7c00) != 0x80
            or len(endpoint.words) + len(WORDS) + 1 > endpoint.WORD_LIMIT):
        raise UnsupportedDevice('AF iris movement requires its idle original homing/option boundary')
    body = tuple(_read(b, 0xc9766c + i*4) for i in range(6))
    cfg = _read(b, 0xbe39fc)
    if (_read(b, 0xc97664) != 1 or body != BODY
            or any(_read(b, a) != v for a, v in ((0xca7bc0, 3 if _from_wire else 0), (0xca7bc8, 2),
                                               (0xca7bd8, 1), (0xca7ca8, 2)))
            or _read(b, 0xca7c20, 2) != 0
            or not 0x40000000 <= cfg < 0x44000000
            or _read(b, cfg + 0x1977e, 1) != 56
            or _read(b, cfg + 0x19712, 1) <= 11):
        raise UnsupportedDevice('AF iris movement requires actual index11 request and calibrated zero-to56 units')
    if _from_wire and (_read(b, 0xca7b68, 2) != 0x4501
            or tuple(_read(b, 0xca7b98+i*4) for i in range(6)) != (0xa09e44, 11, 1, 0, 1, 1)
            or _read(b, 0xca7c22, 2) != 56):
        raise UnsupportedDevice('AF iris automatic admission requires its actual4501 zero-to56 driver')
    banks = b.interrupts.banks
    if any(bank.pending & (1 << 12) or bank.active == 12 or bank.injected == 12 for bank in banks):
        raise UnsupportedDevice('AF iris movement requires the previous physical IRQ12 to be drained')
    owner = endpoint.words[-1]['cpu']
    if type(owner) is not int or owner != 0 or type(b.virtual_ticks) is not int:
        raise UnsupportedDevice('AF iris movement requires a typed CPU0 caller and clock')
    request = dict(profile=PROFILE, owner=owner, armed_tick=b.virtual_ticks,
                   anchor=len(endpoint.words), prefix_sha256=_digest(endpoint.words),
                   prefix_registers=deepcopy(endpoint.registers), body=list(body),
                   state='armed', responses=0, word_indices=[])
    endpoint.requests['iris'] = request
    b.record('Declared AF iris index11 physical input armed', profile=PROFILE,
             limits='One exact1792-quantity command; no RAM, focus counter, GPIO or software callback effects')
    return deepcopy(request)


def arm_from_wire(endpoint):
    return arm(endpoint, PROFILE, _from_wire=True)


def validate(endpoint):
    r, b, f = endpoint.requests['iris'], endpoint.board, _fixture(endpoint)
    if (type(r) is not dict or r.get('profile') != PROFILE or r.get('body') != list(BODY)
            or type(r.get('owner')) is not int or r['owner'] != 0
            or type(r.get('anchor')) is not int or not 0 < r['anchor'] <= len(endpoint.words)
            or type(r.get('armed_tick')) is not int or not 0 <= r['armed_tick'] <= b.virtual_ticks
            or r.get('state') not in ('armed', 'collecting', 'pending-response', 'awaiting-cleanup', 'complete')
            or type(r.get('word_indices')) is not list or type(r.get('responses')) is not int):
        raise UnsupportedDevice('AF iris movement identity/schema changed')
    state, indices, anchor = r['state'], r['word_indices'], r['anchor']
    keys = {'profile', 'owner', 'armed_tick', 'anchor', 'prefix_sha256', 'prefix_registers',
            'body', 'state', 'responses', 'word_indices'}
    if state in ('pending-response', 'awaiting-cleanup', 'complete'):
        keys |= {'configuration', 'requested_tick', 'due_tick'}
    if state in ('awaiting-cleanup', 'complete'): keys.add('delivery_tick')
    if state == 'complete': keys.add('cleanup_index')
    count = len(indices)
    if (set(r) != keys or r['responses'] != int(state in ('awaiting-cleanup', 'complete'))
            or indices != list(range(anchor, anchor+count)) or any(type(i) is not int for i in indices)
            or not 0 <= count <= len(WORDS)
            or (state == 'armed') != (count == 0)
            or (state == 'collecting') != (0 < count < len(WORDS))
            or len(endpoint.words) != anchor+count+int(state == 'complete')
            or _digest(endpoint.words[:anchor]) != r['prefix_sha256']
            or f.words != endpoint.words[:anchor] or f.registers != r['prefix_registers']):
        raise UnsupportedDevice('AF iris movement prefix/order/archive changed')
    events = endpoint.words[anchor:anchor+count]
    if (tuple(e['word'] for e in events) != WORDS[:count]
            or any(type(e['cpu']) is not int or e['cpu'] != r['owner']
                   or type(e['tick']) is not int or not r['armed_tick'] <= e['tick'] <= b.virtual_ticks
                   for e in events)
            or any(a['tick'] > c['tick'] for a, c in zip(events, events[1:]))):
        raise UnsupportedDevice('AF iris movement wire/owner/clock changed')
    regs = dict(r['prefix_registers'])
    for e in events: regs[e['word'] & 0xfc00] = e['word'] & 0x3ff
    if count == len(WORDS):
        config = {w & 0xfc00: w & 0x3ff for w in WORDS}
        quantity = config[0x4400] | config[0x4800] << 10
        if (r['configuration'] != config or quantity != 1792
                or config[0x4000] & 0x200 != 0x200 or config[0x7c00] != 0x2c0
                or type(r['requested_tick']) is not int or r['requested_tick'] != events[-1]['tick']
                or type(r['due_tick']) is not int or r['due_tick'] != r['requested_tick']+endpoint.response_ticks):
            raise UnsupportedDevice('AF iris movement quantity/direction/response clock changed')
    if state in ('awaiting-cleanup', 'complete'):
        if type(r['delivery_tick']) is not int or not r['due_tick'] <= r['delivery_tick'] <= b.virtual_ticks:
            raise UnsupportedDevice('AF iris response delivery clock changed')
    if state == 'complete':
        e = endpoint.words[-1]
        if (type(r['cleanup_index']) is not int or r['cleanup_index'] != anchor+len(WORDS)
                or e['word'] != 0x7cc0 or e['cpu'] != r['owner'] or e['tick'] < r['delivery_tick']):
            raise UnsupportedDevice('AF iris response requires its actual7CC0 cleanup')
        regs[0x7c00] = 0xc0
    if endpoint.registers != regs:
        raise UnsupportedDevice('AF iris movement retained register bank changed')
    return r


def receive(endpoint, core, word, prospective):
    r = validate(endpoint)
    count = len(r['word_indices'])
    expected = WORDS[count] if r['state'] in ('armed', 'collecting') else (
        0x7cc0 if r['state'] == 'awaiting-cleanup' else None)
    if core != r['owner'] or expected is None or word != expected:
        raise UnsupportedDevice('AF iris movement admits only its whole request and post-IRQ cleanup')
    result = deepcopy(r)
    if r['state'] == 'awaiting-cleanup':
        result.update(state='complete', cleanup_index=len(endpoint.words))
    else:
        result['word_indices'].append(len(endpoint.words))
        result['state'] = 'collecting'
        if count+1 == len(WORDS):
            _route(endpoint, core)
            result.update(state='pending-response', configuration={w & 0xfc00: w & 0x3ff for w in WORDS},
                          requested_tick=endpoint.board.virtual_ticks,
                          due_tick=endpoint.board.virtual_ticks+endpoint.response_ticks)
            _note(endpoint, 'iris', phase='measurement', event='start', tick=endpoint.board.virtual_ticks,
                  due_tick=result['due_tick'])
    return result


def _timer_request(endpoint):
    """Inspect only whether this retained lifetime owns a future output.

    Archive/wire validation belongs to consumption, public snapshots and the
    due response. No hash/cache/state field is needed for an inert lifetime.
    """
    r = endpoint.requests['iris']
    if (type(r) is not dict or r.get('profile') != PROFILE
            or r.get('state') not in ('armed', 'collecting', 'pending-response',
                                     'awaiting-cleanup', 'complete')):
        raise UnsupportedDevice('AF iris timer identity/state changed')
    if (r['state'] == 'pending-response'
            and (type(r.get('due_tick')) is not int or not 0 <= r['due_tick'] < 1 << 64)):
        raise UnsupportedDevice('AF iris pending response clock changed')
    return r


def next_event_tick(endpoint):
    r = _timer_request(endpoint)
    return r['due_tick'] if r['state'] == 'pending-response' else None


def tick(endpoint):
    r = _timer_request(endpoint)
    if r['state'] != 'pending-response' or endpoint.board.virtual_ticks < r['due_tick']: return
    r = validate(endpoint)
    _route(endpoint, r['owner'])
    endpoint.board.queue_irq(r['owner'], 12)
    r.update(state='awaiting-cleanup', responses=1, delivery_tick=endpoint.board.virtual_ticks)
    _note(endpoint, 'iris', phase='measurement', event='end', tick=endpoint.board.virtual_ticks)
    endpoint.board.record('Declared AF iris quantity1792 physical response', profile=PROFILE,
                          cpu=r['owner'], irq=12,
                          limits='Stock4509 consumer owns iris position56/index11 and client completion')
