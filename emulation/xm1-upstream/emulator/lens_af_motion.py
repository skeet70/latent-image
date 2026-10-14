"""Opt-in AF lens input lifetimes, selected once by the host scenario.

The original homing and index11 iris response archives remain unchanged. A
recognized later driver owns only its new wire suffix. The zero-travel iris
restore still programs head320 and a strobe, so it requires physical IRQ12;
stock4509/A179D4/4241/12F86CC own closure. Timing is declared, not measured.
"""
from copy import deepcopy
import hashlib
import json
from types import SimpleNamespace

from .hardware import UnsupportedDevice
from .lens_focus_motion import (GENERIC_PROFILE, lens_note, generic_abort_selected, generic_hold_selected, generic_next_tick,
    generic_parameters, generic_receive, generic_selected, generic_tick, generic_validate)

PROFILE = 'declared-afs-parameter-driven-focus-wire-feedback-unmeasured'
LIMIT = 128
AF_IRIS_BODY = (0x12fc3a4, 0, 1, 1, 0, 2)
AF_IRIS_DRIVER_BODY = (0xa09e44, 0, 1, 1, 0, 2)
AF_IRIS_WORDS = tuple(int(w, 16) for w in (
    '7CC0 4014 5440 4E71 584C 5C4C 604C 644C 5280 78D0 '
    '6800 6D40 4700 4801 7E80').split())
RESTORE_BODY = (0x12f86cc, 11, 1, 1, 1, 1)
RESTORE_DRIVER_BODY = (0xa09e44, 11, 1, 1, 1, 1)
RESTORE_WORDS = tuple(int(w, 16) for w in (
    '7CC0 4214 5440 4E71 584C 5C4C 604C 644C 5280 78D0 '
    '6940 6C00 4400 4800 7EC0').split())
FINAL_IRIS_BODY = (0x123a560, 0, 1, 1, 0, 4)
FINAL_IRIS_DRIVER_BODY = (0xa09e44, 0, 1, 1, 0, 4)


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def _read(endpoint, address, size=4):
    return int.from_bytes(endpoint.board.read_memory(address, size), 'little')


def _replay(events, registers):
    registers = dict(registers)
    for event in events:
        registers[event['word'] & 0xfc00] = event['word'] & 0x3ff
    return registers


def selected(endpoint):
    r = endpoint.requests['focus']
    if r is None:
        return False
    if type(r) is dict and r.get('profile') == GENERIC_PROFILE:
        return False
    if type(r) is not dict or r.get('profile') != PROFILE:
        raise UnsupportedDevice('Full lens endpoint has an unknown selected AF input ledger')
    return True


def enable_generic(endpoint, **parameters):
    """Select any stock A08F84/4401 and A08084/4403 focus move; iris moves stay refused."""
    from .lens_endpoint import HOMING_PROFILE
    from .lens_measurement_iris import _fixture
    params = generic_parameters(**parameters)
    if (endpoint.response_profile != HOMING_PROFILE or endpoint.requests['focus'] is not None
            or endpoint.requests['iris'] is not None or endpoint.board.failure is not None
            or type(endpoint.board.virtual_ticks) is not int):
        raise UnsupportedDevice('Generic focus moves require one explicit selection on the idle full lens profile')
    _fixture(endpoint)
    if endpoint.words != endpoint.homing_fixture.words or endpoint.registers != endpoint.homing_fixture.registers:
        raise UnsupportedDevice('Generic focus moves require the unchanged original lens input archive')
    endpoint.requests['focus'] = dict(profile=GENERIC_PROFILE, state='generic',
        enabled_tick=endpoint.board.virtual_ticks, anchor=len(endpoint.words),
        prefix_sha256=_digest(endpoint.words), params=params, records=[], iris=[])
    return deepcopy(endpoint.requests['focus'])


def validate_generic(endpoint):
    r = endpoint.requests['focus']
    if (type(r) is not dict or r.get('profile') != GENERIC_PROFILE or r.get('state') != 'generic'
            or set(r) != {'profile', 'state', 'enabled_tick', 'anchor', 'prefix_sha256', 'params', 'records', 'iris'}
            or r['params'] != generic_parameters(**r['params']) or type(r['anchor']) is not int
            or not 0 < r['anchor'] <= len(endpoint.words)
            or _digest(endpoint.words[:r['anchor']]) != r['prefix_sha256']
            or type(r['enabled_tick']) is not int or not 0 <= r['enabled_tick'] <= endpoint.board.virtual_ticks):
        raise UnsupportedDevice('Generic focus immutable prefix/schema changed')
    return generic_validate(endpoint, r)


def receive_generic(endpoint, core, word):
    r = validate_generic(endpoint)
    return 'focus', generic_receive(endpoint, r, core, word)


def next_event_tick_generic(endpoint):
    return generic_next_tick(endpoint, validate_generic(endpoint))


def tick_generic(endpoint):
    generic_tick(endpoint, validate_generic(endpoint))


def _prior_iris(endpoint):
    """Validate the original closed input through an immutable prefix view."""
    from .lens_measurement_iris import validate as validate_iris
    r = endpoint.requests['iris']
    if (type(r) is not dict or r.get('state') != 'complete'
            or type(r.get('cleanup_index')) is not int):
        raise UnsupportedDevice('AF motion requires the completed original iris archive')
    end = r['cleanup_index'] + 1
    events = endpoint.words[:end]
    view = SimpleNamespace(board=endpoint.board, homing_fixture=endpoint.homing_fixture,
        requests=endpoint.requests, words=events, response_ticks=endpoint.response_ticks,
        registers=_replay(events, r['prefix_registers']))
    validate_iris(view)
    return end


def enable(endpoint, profile):
    from .lens_endpoint import HOMING_PROFILE
    from .lens_measurement_iris import _fixture
    if (profile != PROFILE or endpoint.response_profile != HOMING_PROFILE
            or endpoint.requests['focus'] is not None or endpoint.board.failure is not None
            or type(endpoint.board.virtual_ticks) is not int):
        raise UnsupportedDevice('AF motion requires one explicit named selection on the full lens profile')
    _fixture(endpoint)
    if endpoint.requests['iris'] is not None:
        if _prior_iris(endpoint) != len(endpoint.words):
            raise UnsupportedDevice('AF motion selection requires the exact completed iris boundary')
    elif endpoint.words != endpoint.homing_fixture.words or endpoint.registers != endpoint.homing_fixture.registers:
        raise UnsupportedDevice('AF motion selection requires its unchanged original lens input archive')
    endpoint.requests['focus'] = dict(profile=PROFILE, state='waiting-for-iris',
        enabled_tick=endpoint.board.virtual_ticks)
    return deepcopy(endpoint.requests['focus'])


def _timer(endpoint):
    r = endpoint.requests['focus']
    if (type(r) is not dict or r.get('profile') != PROFILE or r.get('state') not in
            ('waiting-for-iris', 'collecting', 'pending-response', 'awaiting-cleanup', 'complete', 'af-iris')
            or type(r.get('enabled_tick')) is not int
            or not 0 <= r['enabled_tick'] <= endpoint.board.virtual_ticks):
        raise UnsupportedDevice('AF motion retained selection/timer identity changed')
    if r['state'] == 'pending-response' or (r['state'] == 'af-iris' and
            type(r.get('current')) is dict and r['current'].get('state') == 'pending-response'):
        current = r.get('current')
        if (type(current) is not dict or type(current.get('due_tick')) is not int
                or not 0 <= current['due_tick'] < 1 << 64):
            raise UnsupportedDevice('AF motion retained response clock changed')
    return r


def validate(endpoint):
    r = _timer(endpoint)
    if r['state'] == 'af-iris':
        return _validate_af_iris(endpoint, r)
    if r['state'] == 'waiting-for-iris':
        if set(r) != {'profile', 'state', 'enabled_tick'}:
            raise UnsupportedDevice('AF motion waiting selection schema changed')
        if endpoint.requests['iris'] is not None:
            from .lens_measurement_iris import validate as validate_iris
            validate_iris(endpoint)
        elif endpoint.words != endpoint.homing_fixture.words or endpoint.registers != endpoint.homing_fixture.registers:
            raise UnsupportedDevice('AF motion waiting archive changed')
        return r
    if (set(r) != {'profile', 'state', 'enabled_tick', 'anchor', 'prefix_sha256',
                    'prefix_registers', 'current', 'history'}
            or type(r['anchor']) is not int or _prior_iris(endpoint) != r['anchor']
            or type(r['history']) is not list or len(r['history']) > LIMIT
            or _digest(endpoint.words[:r['anchor']]) != r['prefix_sha256']
            or _replay(endpoint.words[r['anchor']:], r['prefix_registers']) != endpoint.registers):
        raise UnsupportedDevice('AF motion immutable prefix/schema/register bank changed')
    # Current supported law owns a single restore. Future driver families must
    # declare their own admission and wire law before extending this history.
    if r['history']:
        raise UnsupportedDevice('AF motion archived later driver law remains unsupported')
    m = r['current']
    if (type(m) is not dict or m.get('kind') != 'iris-index11-zero-travel'
            or m.get('body') != list(RESTORE_BODY) or m.get('start') != r['anchor']
            or type(m.get('count')) is not int or not 1 <= m['count'] <= len(RESTORE_WORDS)
            or type(m.get('tick')) is not int or not r['enabled_tick'] <= m['tick'] <= endpoint.board.virtual_ticks
            or type(m.get('responses')) is not int
            or m['responses'] != int(r['state'] in ('awaiting-cleanup', 'complete'))):
        raise UnsupportedDevice('AF iris restore retained driver/count/clock changed')
    count, start = m['count'], m['start']
    events = endpoint.words[start:start+count]
    if (len(endpoint.words) != start+count+int(r['state'] == 'complete')
            or tuple(e['word'] for e in events) != RESTORE_WORDS[:count]
            or any(type(e.get('cpu')) is not int or e['cpu'] != 0
                   or type(e.get('tick')) is not int or not m['tick'] <= e['tick'] <= endpoint.board.virtual_ticks
                   for e in events)
            or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))
            or _digest(events) != m.get('wire_sha256')
            or (r['state'] == 'collecting') != (count < len(RESTORE_WORDS))):
        raise UnsupportedDevice('AF iris restore retained wire/owner/order changed')
    keys = {'kind', 'body', 'start', 'tick', 'count', 'responses', 'wire_sha256'}
    if count == len(RESTORE_WORDS):
        keys |= {'requested_tick', 'due_tick', 'configuration'}
        cfg = {w & 0xfc00: w & 0x3ff for w in RESTORE_WORDS}
        if (m.get('configuration') != cfg or cfg[0x4400] | cfg[0x4800] << 10 != 0
                or cfg[0x6800] != 320 or cfg[0x6c00] != 0
                or type(m.get('requested_tick')) is not int or m['requested_tick'] != events[-1]['tick']
                or type(m.get('due_tick')) is not int
                or m['due_tick'] != m['requested_tick']+endpoint.response_ticks):
            raise UnsupportedDevice('AF iris zero quantity/head/response clock changed')
    if r['state'] in ('awaiting-cleanup', 'complete'):
        keys.add('delivery_tick')
        if type(m.get('delivery_tick')) is not int or not m['due_tick'] <= m['delivery_tick'] <= endpoint.board.virtual_ticks:
            raise UnsupportedDevice('AF iris restore delivery clock changed')
    if r['state'] == 'complete':
        keys.add('cleanup_index')
        e = endpoint.words[-1]
        if (m.get('cleanup_index') != start+len(RESTORE_WORDS)
                or e['word'] != 0x7cc0 or e['cpu'] != 0 or e['tick'] < m['delivery_tick']):
            raise UnsupportedDevice('AF iris restore actual cleanup changed')
    if set(m) != keys:
        raise UnsupportedDevice('AF iris restore retained schema changed')
    return r


def _restore_admission(endpoint):
    b = endpoint.board
    cfg = _read(endpoint, 0xbe39fc)
    if (b.failure is not None or _read(endpoint, 0xc97664) != 1
            or tuple(_read(endpoint, 0xc9766c+i*4) for i in range(6)) != RESTORE_BODY
            or _read(endpoint, 0xca7b68, 2) != 0x4501
            or tuple(_read(endpoint, 0xca7b74+i*4) for i in range(6)) != RESTORE_DRIVER_BODY
            or tuple(_read(endpoint, 0xca7b98+i*4) for i in range(6)) != RESTORE_DRIVER_BODY
            or any(_read(endpoint, a) != v for a, v in ((0xca7bc0, 3), (0xca7bc8, 2), (0xca7bd8, 1)))
            or _read(endpoint, 0xca7c20, 2) != 56 or _read(endpoint, 0xca7c22, 2) != 0
            or _read(endpoint, 0xca7cb0, 1) != 11
            or not 0x40000000 <= cfg < 0x44000000 or _read(endpoint, cfg+0x1977e, 1) != 56
            or any(bank.pending & (1 << 12) or bank.active == 12 or bank.injected == 12
                   for bank in b.interrupts.banks)):
        raise UnsupportedDevice('AF iris restore requires its actual zero-travel4213/4501 caller')


def receive(endpoint, core, word, prospective):
    """Return (ledger axis, new record); transport commits the accepted word."""
    r = validate(endpoint)
    if r['state'] == 'af-iris':
        return 'focus', _receive_af_iris(endpoint, r, core, word)
    if r['state'] == 'complete' and word == AF_IRIS_WORDS[0]:
        return 'focus', _receive_af_iris(endpoint, _begin_af_iris(endpoint, r), core, word)
    if core != 0:
        raise UnsupportedDevice('AF motion requires its actual CPU0 driver')
    if r['state'] == 'waiting-for-iris':
        iris = endpoint.requests['iris']
        if iris is None:
            from .lens_measurement_iris import BODY, arm_from_wire, receive as receive_iris
            body = tuple(_read(endpoint, 0xc9766c+i*4) for i in range(6))
            if _read(endpoint, 0xc97664) == 1 and body == BODY and word == 0x7c80:
                arm_from_wire(endpoint)
                return 'iris', receive_iris(endpoint, core, word, prospective)
            endpoint.homing_fixture.receive(core, word, prospective)
            return None, None
        if iris['state'] != 'complete':
            from .lens_measurement_iris import receive as receive_iris
            return 'iris', receive_iris(endpoint, core, word, prospective)
        if word != RESTORE_WORDS[0]:
            raise UnsupportedDevice('AF motion later lens driver class remains unresolved')
        _restore_admission(endpoint)
        anchor = _prior_iris(endpoint)
        result = dict(profile=PROFILE, state='collecting', enabled_tick=r['enabled_tick'],
            anchor=anchor, prefix_sha256=_digest(endpoint.words), prefix_registers=deepcopy(endpoint.registers),
            history=[], current=dict(kind='iris-index11-zero-travel', body=list(RESTORE_BODY),
                start=anchor, tick=endpoint.board.virtual_ticks, count=0, responses=0))
    else:
        result = deepcopy(r)
    m = result['current']
    expected = RESTORE_WORDS[m['count']] if result['state'] == 'collecting' else (
        0x7cc0 if result['state'] == 'awaiting-cleanup' else None)
    if expected is None or word != expected:
        raise UnsupportedDevice('AF iris restore admits only its whole zero-travel command and post-IRQ cleanup')
    if result['state'] == 'awaiting-cleanup':
        result['state'] = 'complete'; m['cleanup_index'] = len(endpoint.words)
    else:
        m['count'] += 1
        events = endpoint.words[m['start']:]+[dict(cpu=core, word=word, tick=endpoint.board.virtual_ticks)]
        m['wire_sha256'] = _digest(events)
        if m['count'] == len(RESTORE_WORDS):
            result['state'] = 'pending-response'
            m.update(configuration={w & 0xfc00: w & 0x3ff for w in RESTORE_WORDS},
                requested_tick=endpoint.board.virtual_ticks,
                due_tick=endpoint.board.virtual_ticks+endpoint.response_ticks)
            lens_note(endpoint, 'iris', phase='restore-index11', event='start',
                      tick=endpoint.board.virtual_ticks, due_tick=m['due_tick'])
    return 'focus', result


def next_event_tick(endpoint):
    r = _timer(endpoint)
    if r['state'] == 'af-iris':
        from .lens_focus_motion import next_event_tick as motor_due
        times=[r['current']['due_tick']] if r['current']['state']=='pending-response' else []
        motor=motor_due(endpoint,r.get('motor'))
        if motor is not None:times.append(motor)
        return min(times) if times else None
    return r['current']['due_tick'] if r['state'] == 'pending-response' else None


def tick(endpoint):
    due = next_event_tick(endpoint)
    if due is None or endpoint.board.virtual_ticks < due:
        return
    r = validate(endpoint); m = r['current']
    if r['state'] == 'af-iris':
        from .lens_focus_motion import next_event_tick as motor_due,validate as validate_motor,tick as tick_motor
        m=r['current'];motor=r.get('motor');mdue=motor_due(endpoint,motor)
        iris_due=m['state']=='pending-response' and endpoint.board.virtual_ticks>=m['due_tick']
        if iris_due:_check_af_iris(endpoint,r)
        if mdue is not None and endpoint.board.virtual_ticks>=mdue:validate_motor(endpoint,motor,pending=True)
        if iris_due:_tick_af_iris(endpoint,r)
        if mdue is not None and endpoint.board.virtual_ticks>=mdue:tick_motor(endpoint,motor)
        return
    from .lens_measurement_iris import _route
    _route(endpoint, 0)
    if (_read(endpoint, 0xca7b8c, 2) != 0x4501
            or tuple(_read(endpoint, 0xca7b98+i*4) for i in range(6)) != RESTORE_DRIVER_BODY
            or _read(endpoint, 0xca7bc0) != 3 or _read(endpoint, 0xca7bc8) != 3
            or _read(endpoint, 0xca7c20, 2) != 56 or _read(endpoint, 0xca7cb0, 1) != 11):
        raise UnsupportedDevice('AF iris restore native request ownership changed before response')
    endpoint.board.queue_irq(0, 12)
    r['state'] = 'awaiting-cleanup'
    m.update(responses=1, delivery_tick=endpoint.board.virtual_ticks)
    lens_note(endpoint, 'iris', phase='restore-index11', event='end', tick=endpoint.board.virtual_ticks)
    endpoint.board.record('Declared AF iris zero-travel physical response', profile=PROFILE,
        irq=12, limits='Head320 and quantity0; no motor edges, guest writes or callback/result substitutions')


def _begin_af_iris(endpoint, old):
    b = endpoint.board
    if (_read(endpoint, 0xc97664) != 1
            or tuple(_read(endpoint, 0xc9766c+i*4) for i in range(6)) != AF_IRIS_BODY
            or _read(endpoint, 0xca7b68, 2) != 0x4501
            or tuple(_read(endpoint, 0xca7b74+i*4) for i in range(6)) != AF_IRIS_DRIVER_BODY
            or tuple(_read(endpoint, 0xca7b98+i*4) for i in range(6)) != AF_IRIS_DRIVER_BODY
            or any(_read(endpoint, a) != v for a, v in ((0xca7bc0, 3), (0xca7bc8, 2), (0xca7bd8, 1)))
            or _read(endpoint, 0xca7c20, 2) != 56 or _read(endpoint, 0xca7c22, 2) != 0xffc8
            or _read(endpoint, 0xca7cb0, 1) != 11
            or any(bank.pending & (1 << 12) or bank.active == 12 or bank.injected == 12
                   for bank in b.interrupts.banks)):
        raise UnsupportedDevice('Contrast AF iris requires actual4213/4501 index0 and accepted delta-56')
    return dict(profile=PROFILE, state='af-iris', enabled_tick=old['enabled_tick'],
        anchor=len(endpoint.words), prefix_sha256=_digest(endpoint.words),
        prefix_registers=deepcopy(endpoint.registers), history=[deepcopy(old)],
        current=dict(kind='contrast-iris-index0', state='collecting', body=list(AF_IRIS_BODY),
            start=len(endpoint.words), tick=b.virtual_ticks, count=0, responses=0))


def _validate_af_iris(endpoint, r):
    keys={'profile','state','enabled_tick','anchor','prefix_sha256','prefix_registers','history','current'}
    if 'motor' in r:keys.add('motor')
    if 'iris_closure' in r:keys.add('iris_closure')
    if (set(r) != keys
            or type(r['anchor']) is not int or not 0 < r['anchor'] <= len(endpoint.words)
            or type(r['history']) is not list or len(r['history']) != 1
            or r['history'][0].get('state') != 'complete'
            or _digest(endpoint.words[:r['anchor']]) != r['prefix_sha256']
            or _replay(endpoint.words[r['anchor']:], r['prefix_registers']) != endpoint.registers):
        raise UnsupportedDevice('Contrast AF iris immutable restore prefix/schema changed')
    view = SimpleNamespace(board=endpoint.board, homing_fixture=endpoint.homing_fixture,
        requests=dict(endpoint.requests, focus=r['history'][0]), words=endpoint.words[:r['anchor']],
        registers=r['prefix_registers'], response_ticks=endpoint.response_ticks)
    validate(view)  # The original closed restore owns only its immutable prefix.
    m = r['current']
    if (type(m) is not dict or m.get('kind') != 'contrast-iris-index0'
            or m.get('body') != list(AF_IRIS_BODY) or m.get('start') != r['anchor']
            or m.get('state') not in ('collecting', 'pending-response', 'awaiting-cleanup', 'complete')
            or type(m.get('count')) is not int or not 1 <= m['count'] <= len(AF_IRIS_WORDS)
            or type(m.get('tick')) is not int or not r['enabled_tick'] <= m['tick'] <= endpoint.board.virtual_ticks
            or type(m.get('responses')) is not int
            or m['responses'] != int(m['state'] in ('awaiting-cleanup', 'complete'))):
        raise UnsupportedDevice('Contrast AF iris retained driver/count/clock changed')
    count, start = m['count'], m['start']; events = endpoint.words[start:start+count]
    if (tuple(e['word'] for e in events) != AF_IRIS_WORDS[:count]
            or any(type(e.get('cpu')) is not int or e['cpu'] != 0
                   or type(e.get('tick')) is not int or not m['tick'] <= e['tick'] <= endpoint.board.virtual_ticks
                   for e in events)
            or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))
            or _digest(events) != m.get('wire_sha256')
            or (m['state'] == 'collecting') != (count < len(AF_IRIS_WORDS))):
        raise UnsupportedDevice('Contrast AF iris retained wire/owner/order changed')
    keys = {'kind', 'state', 'body', 'start', 'tick', 'count', 'responses', 'wire_sha256'}
    if count == len(AF_IRIS_WORDS):
        keys |= {'requested_tick', 'due_tick', 'configuration'}
        cfg = {w & 0xfc00: w & 0x3ff for w in AF_IRIS_WORDS}
        if (m.get('configuration') != cfg or cfg[0x4400] | cfg[0x4800] << 10 != 1792
                or cfg[0x6800] != 0 or cfg[0x6c00] != 320
                or type(m.get('requested_tick')) is not int or m['requested_tick'] != events[-1]['tick']
                or type(m.get('due_tick')) is not int
                or m['due_tick'] != m['requested_tick']+endpoint.response_ticks):
            raise UnsupportedDevice('Contrast AF iris quantity/tail/response clock changed')
    if m['state'] in ('awaiting-cleanup', 'complete'):
        keys.add('delivery_tick')
        if type(m.get('delivery_tick')) is not int or not m['due_tick'] <= m['delivery_tick'] <= endpoint.board.virtual_ticks:
            raise UnsupportedDevice('Contrast AF iris delivery clock changed')
    if m['state'] == 'complete':
        keys.add('cleanup_index')
        if type(m.get('cleanup_index')) is not int or not start+len(AF_IRIS_WORDS)<=m['cleanup_index']<len(endpoint.words):
            raise UnsupportedDevice('Contrast AF iris cleanup ownership changed')
        e = endpoint.words[m['cleanup_index']]
        if (e['word'] != 0x7c00 or e['cpu'] != 0 or e['tick'] < m['delivery_tick']):
            raise UnsupportedDevice('Contrast AF iris actual cleanup changed')
    if set(m) != keys:
        raise UnsupportedDevice('Contrast AF iris retained schema changed')
    from .lens_focus_motion import validate as validate_motor,owned_indices
    motor=r.get('motor')
    if motor is not None:
        pending=type(motor.get('current')) is dict and motor['current'].get('state')=='pending-response'
        validate_motor(endpoint,motor,pending=pending)
    indices=list(range(start,start+count))
    if m['state']=='complete':indices.append(m['cleanup_index'])
    indices+=owned_indices(motor)
    if 'iris_closure' in r:
        close = r['iris_closure']
        if (m['state'] != 'complete' or type(close) is not dict
                or set(close) != {'kind', 'body', 'driver_body', 'word_index', 'tick',
                                  'prefix_sha256', 'motor_sha256'}
                or close['kind'] != 'contrast-final-iris-index0-no-travel'
                or close['body'] != list(FINAL_IRIS_BODY)
                or close['driver_body'] != list(FINAL_IRIS_DRIVER_BODY)
                or type(close['word_index']) is not int
                or close['word_index'] != len(endpoint.words)-1
                or type(close['tick']) is not int
                or not m['delivery_tick'] <= close['tick'] <= endpoint.board.virtual_ticks
                or _digest(endpoint.words[:close['word_index']]) != close['prefix_sha256']
                or _digest(motor) != close['motor_sha256']
                or endpoint.words[close['word_index']] != dict(cpu=0, word=0x7c00, tick=close['tick'])):
            raise UnsupportedDevice('Final contrast iris passive closure identity/history changed')
        _closed_final_motor(motor)
        indices.append(close['word_index'])
    if len(indices)!=len(set(indices)) or sorted(indices)!=list(range(r['anchor'],len(endpoint.words))):
        raise UnsupportedDevice('AF iris/motor wire ownership changed')
    return r


def _receive_af_iris(endpoint, record, core, word):
    result = deepcopy(record); m = result['current']
    if core == 0 and word == 0x7c00 and m['state'] == 'complete':
        result['iris_closure'] = _final_iris_closure(endpoint, result)
        return result
    if word<0x4000 or word>=0xc000:
        from .lens_focus_motion import receive as receive_motor
        result['motor']=receive_motor(endpoint,result.get('motor'),core,word)
        return result
    expected = AF_IRIS_WORDS[m['count']] if m['state'] == 'collecting' else (
        0x7c00 if m['state'] == 'awaiting-cleanup' else None)
    if core != 0 or expected is None or word != expected:
        raise UnsupportedDevice('Contrast AF iris admits only the whole index0 request and post-IRQ cleanup')
    if m['state'] == 'awaiting-cleanup':
        m.update(state='complete', cleanup_index=len(endpoint.words))
    else:
        m['count'] += 1
        events = endpoint.words[m['start']:]+[dict(cpu=core, word=word, tick=endpoint.board.virtual_ticks)]
        m['wire_sha256'] = _digest(events)
        if m['count'] == len(AF_IRIS_WORDS):
            m.update(state='pending-response', configuration={w & 0xfc00: w & 0x3ff for w in AF_IRIS_WORDS},
                requested_tick=endpoint.board.virtual_ticks,
                due_tick=endpoint.board.virtual_ticks+endpoint.response_ticks)
            lens_note(endpoint, 'iris', phase='contrast-index0', event='start',
                      tick=endpoint.board.virtual_ticks, due_tick=m['due_tick'])
    return result


def _closed_final_motor(motor):
    from .lens_focus_motion import _fitted_body
    current = motor.get('current') if type(motor) is dict else None
    if (type(current) is not dict or current.get('state') != 'complete-without-timer'
            or not _fitted_body(current.get('body'))
            or type(current.get('continuation')) is not dict
            or current['continuation'].get('state') != 0x14):
        raise UnsupportedDevice('Final contrast iris requires the completed fitted focus wire owner')


def _final_iris_closure(endpoint, record):
    """One actual zero-travel4501 cleanup; it owns no response or timer."""
    if 'iris_closure' in record:
        raise UnsupportedDevice('Final contrast iris passive closure is already complete')
    _closed_final_motor(record.get('motor'))
    if (_read(endpoint, 0xc97664) != 1
            or tuple(_read(endpoint, 0xc9766c+i*4) for i in range(6)) != FINAL_IRIS_BODY
            or _read(endpoint, 0xca7b68, 2) != 0x4501
            or _read(endpoint, 0xca7b8c, 2) != 0x4501
            or tuple(_read(endpoint, 0xca7b74+i*4) for i in range(6)) != FINAL_IRIS_DRIVER_BODY
            or tuple(_read(endpoint, 0xca7b98+i*4) for i in range(6)) != FINAL_IRIS_DRIVER_BODY
            or any(_read(endpoint, a) != v for a, v in (
                (0xca7bb4, 1), (0xca7bb8, 0), (0xca7bc0, 3), (0xca7bc4, 0),
                (0xca7bc8, 0), (0xca7bcc, 1), (0xca7bd8, 1), (0xca7c14, 3)))
            or _read(endpoint, 0xca7c20, 2) != 0 or _read(endpoint, 0xca7c22, 2) != 0
            or _read(endpoint, 0xca7cb8, 2) != 0 or _read(endpoint, 0xca7cb0, 1) != 31
            or any(bank.pending & (1 << 12) or bank.active == 12 or bank.injected == 12
                   for bank in endpoint.board.interrupts.banks)):
        raise UnsupportedDevice('Final contrast iris requires its actual zero-travel4213/4501 caller')
    return dict(kind='contrast-final-iris-index0-no-travel', body=list(FINAL_IRIS_BODY),
        driver_body=list(FINAL_IRIS_DRIVER_BODY), word_index=len(endpoint.words),
        tick=endpoint.board.virtual_ticks, prefix_sha256=_digest(endpoint.words),
        motor_sha256=_digest(record['motor']))


def _check_af_iris(endpoint, r):
    m = r['current']
    from .lens_measurement_iris import _route
    _route(endpoint, 0)
    if (_read(endpoint, 0xca7b8c, 2) != 0x4501
            or tuple(_read(endpoint, 0xca7b98+i*4) for i in range(6)) != AF_IRIS_DRIVER_BODY
            or _read(endpoint, 0xca7bc0) != 3 or _read(endpoint, 0xca7bc8) != 3
            or _read(endpoint, 0xca7c20, 2) != 56 or _read(endpoint, 0xca7cb0, 1) != 11):
        raise UnsupportedDevice('Contrast AF iris native request ownership changed before response')


def _tick_af_iris(endpoint,r):
    _check_af_iris(endpoint,r)
    m=r['current']
    endpoint.board.queue_irq(0, 12)
    m.update(state='awaiting-cleanup', responses=1, delivery_tick=endpoint.board.virtual_ticks)
    lens_note(endpoint, 'iris', phase='contrast-index0', event='end', tick=endpoint.board.virtual_ticks)
    endpoint.board.record('Declared contrast AF iris physical response', profile=PROFILE,
        irq=12, limits='Wire1792 in negative direction; stock ISR/consumer own pose/index and callback')


def af_motor_abort_selected(board):
    """The generic focus ledger is in the cleanup of a stock-aborted (D801) move: its C000 timer count is free."""
    endpoints=[d for d in board.devices if type(d).__name__=='LensEndpoint']
    return len(endpoints)==1 and generic_selected(endpoints[0]) and generic_abort_selected(endpoints[0].requests['focus'])


def af_motor_timer_selected(board):
    """Exact opt-in guard for the existing pulse device's C001 holdtimer."""
    endpoints=[d for d in board.devices if type(d).__name__=='LensEndpoint']
    if len(endpoints)!=1:return False
    e=endpoints[0]
    if generic_selected(e):return generic_hold_selected(e.requests['focus'])
    if not selected(e):return False
    r=e.requests['focus'];motor=r.get('motor')
    if r.get('state')!='af-iris' or type(motor) is not dict or type(motor.get('current')) is not dict:return False
    return motor['current'].get('state') in ('awaiting-feedback-cleanup','awaiting-stock-close')
