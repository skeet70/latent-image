"""One declared source0 clock for the observed post-AF REC preview restart.

The clock supplies physical IRQ27 only. Stock9DF0B8/13012E4 own3F01 and
selector1 ACK; stock13017A8 owns new preview/statistics programming. There
is no image, source1 edge, AF result, task callback or guest-memory write.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-afs-generation7-preview-return-source0-clock-unmeasured'
KEY = 'preview_return'
IRQ = 0x27
WORDS = ((0x15e32d0, 4, 9), (0x17b4fec, 4, 0), (0x1612d44, 4, 2),
         (0x173d308, 4, 0x3c), (0x15f6e50, 4, 2), (0x15f64a4, 4, 0),
         (0x15f68a4, 4, 0), (0x15f68a8, 4, 0), (0x15f6bf4, 4, 0),
         (0x15f6a60, 4, 0), (0x15f6e48, 4, 0), (0x15e692c, 4, 0),
         (0x15f87f0, 2, 10), (0x15f87f6, 2, 8), (0x15f8812, 2, 1),
         (0x1612d78, 4, 1), (0x1612da4, 4, 0x124ce08),
         (0x1612dc0, 4, 1), (0x1612de0, 4, 0), (0x1612de4, 4, 0),
         (0x1613010, 4, 0), (0x1613014, 4, 0), (0x16130ec, 4, 1),
         (0x1625f5c, 4, 2))
FIELDS = {'profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
          'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'old_source_sha256',
          'input_sha256', 'state_sha256', 'state', 'asserted_tick', 'acknowledged_tick',
          'eoi_tick', 'write_cursor', 'assertion_write_index', 'ack_write_index', 'canceled_tick', 'reason'}
IDENTITY = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
            'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'old_source_sha256')


def _u(source, address, size=4):
    raw = bytes(source.board.read_memory(address, size, cpu_id=0))
    if len(raw) != size:
        raise UnsupportedDevice('Preview-return clock caller RAM is truncated')
    return int.from_bytes(raw, 'little')


def _marker(source):
    return source.request.get(KEY) if type(source.request) is dict else None


def _old(source):
    return {key: value for key, value in source.request.items() if key != KEY}


def _seal(record):
    record['state_sha256'] = digest({key: value for key, value in record.items() if key != 'state_sha256'})


def _drained(source):
    return all(not bank.pending & (1 << IRQ) and bank.active != IRQ and bank.injected != IRQ
               for bank in source.board.interrupts.banks)


def _route(source, inputs):
    bank = source.board.interrupts.banks[0]
    if (inputs._event_route(IRQ) != 0 or not _drained(source)
            or bank.config.get(0x400 + 4 * IRQ) != 0x9df0b8
            or bank.config.get(0x800 + 4 * IRQ) != 10
            or not bank.source_config & (1 << IRQ)):
        raise UnsupportedDevice('Preview-return clock lacks its drained CPU0 source0 route')
    return {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}


def _terminal(source):
    stream = source._validate_af_stream()
    if (type(stream) is not dict or type(stream.get('records')) is not list
            or len(stream['records']) != 6
            or any(record.get('consumed_tick') is None or record.get('canceled_tick') is not None
                   for record in stream['records'])
            or (stream['records'][-1].get('kind'), stream['records'][-1].get('pair_id')) != ('pixel', 5)):
        raise UnsupportedDevice('Preview-return clock requires the closed five-pair AF source history')
    return digest(_old(source))


def _live(source):
    inputs = source._prefix(historical=True)
    if (inputs.event_enable_generation != 7 or inputs.registers.get(0x38c) != 0x10003
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.registers.get(0x70) != 0 or inputs.registers.get(0x154) != 0
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Preview-return clock input gates differ from the idle generation7 owner')
    if any(_u(source, address, size) != value for address, size, value in WORDS):
        raise UnsupportedDevice('Preview-return clock belongs to another sensor/preview/lower17 owner')
    callbacks = []
    for number, expected in ((0, ((0x15f6e5c, 0x11f6008, 2), (0x1612db0, 0x13012e4, 1))),
                             (1, ((0x1612db4, 0x12fee90, 0),))):
        base = 0xc6dc58 + number * 64
        tokens = [_u(source, base + 4 + 4 * index) for index in range(len(expected))]
        if (_u(source, base, 1) != len(expected) or len(set(tokens)) != len(tokens)
                or any(not token or token != _u(source, address)
                       or (_u(source, base + 24 + 8 * index), _u(source, base + 28 + 8 * index)) != (callback, priority)
                       for index, (token, (address, callback, priority)) in enumerate(zip(tokens, expected)))):
            raise UnsupportedDevice('Preview-return clock subscriber handles/callbacks differ from their stock owner')
        callbacks.append({'source': number, 'tokens': tokens,
                          'callbacks': [[callback, priority] for _, callback, priority in expected]})
    if len(set(callbacks[0]['tokens'] + callbacks[1]['tokens'])) != 3:
        raise UnsupportedDevice('Preview-return clock source handles are aliased')
    return inputs, {'generation': 7, 'mask': 0x10003, 'words': [list(word) for word in WORDS],
                    'callbacks': callbacks, 'route': _route(source, inputs),
                    'prefix_sha256': digest(inputs.event_history)}


def arm(source, profile, *, cadence_ticks=1000):
    if (profile != PROFILE or source.board.failure is not None or _marker(source) is not None
            or type(cadence_ticks) is not int or not 1 <= cadence_ticks <= 400000):
        raise UnsupportedDevice('Preview-return clock requires its named one-edge profile and bounded delay')
    source._validate()
    old_sha = _terminal(source)
    inputs, contract = _live(source)
    now = source.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - cadence_ticks:
        raise UnsupportedDevice('Preview-return clock virtual time is outside its integer range')
    record = dict(profile=profile, generation=7, cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                  requested_tick=now, due_tick=now + cadence_ticks, cadence_ticks=cadence_ticks,
                  contract=contract, old_source_sha256=old_sha, state='waiting', asserted_tick=None,
                  acknowledged_tick=None, eoi_tick=None, write_cursor=len(inputs.writes),
                  assertion_write_index=None, ack_write_index=None,
                  canceled_tick=None, reason=None)
    record['input_sha256'] = digest({key: record[key] for key in IDENTITY})
    _seal(record)
    source.request[KEY] = record
    source.board.record('Post-AF REC preview-return clock explicitly armed', profile=profile,
                        due_tick=record['due_tick'], limits='One source0 IRQ; no source1, image, result or callback')
    return deepcopy(record)


def validate(source):
    record = _marker(source)
    if record is None:
        return None
    if (type(record) is not dict or set(record) != FIELDS
            or tuple(record[key] for key in ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
               (PROFILE, 7, 0, 0, '0x27', 1)
            or any(type(record[key]) is not int for key in ('requested_tick', 'due_tick', 'cadence_ticks', 'write_cursor'))
            or not 1 <= record['cadence_ticks'] <= 400000
            or not 0 <= record['requested_tick'] < record['due_tick'] == record['requested_tick'] + record['cadence_ticks'] < 1 << 64
            or record['requested_tick'] > source.board.virtual_ticks
            or record['input_sha256'] != digest({key: record[key] for key in IDENTITY})
            or record['state_sha256'] != digest({key: value for key, value in record.items() if key != 'state_sha256'})
            or record['old_source_sha256'] != _terminal(source)
            or record['state'] not in ('waiting', 'asserted', 'acknowledged', 'canceled')):
        raise UnsupportedDevice('Preview-return clock identity, history, timer or plain ledger changed')
    contract = record['contract']
    if (type(contract) is not dict
            or set(contract) != {'generation', 'mask', 'words', 'callbacks', 'route', 'prefix_sha256'}
            or contract['generation'] != 7 or contract['mask'] != 0x10003
            or contract['words'] != [list(word) for word in WORDS]
            or contract['route'] != {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}
            or type(contract['callbacks']) is not list or len(contract['callbacks']) != 2):
        raise UnsupportedDevice('Preview-return retained owner/route template changed')
    for number, expected in ((0, [[0x11f6008, 2], [0x13012e4, 1]]), (1, [[0x12fee90, 0]])):
        callbacks = contract['callbacks'][number]
        if (type(callbacks) is not dict or set(callbacks) != {'source', 'tokens', 'callbacks'}
                or callbacks['source'] != number or callbacks['callbacks'] != expected
                or type(callbacks['tokens']) is not list or len(callbacks['tokens']) != len(expected)
                or any(type(token) is not int or not 0 < token < 1 << 32 for token in callbacks['tokens'])):
            raise UnsupportedDevice('Preview-return retained subscriber template changed')
    tokens = contract['callbacks'][0]['tokens'] + contract['callbacks'][1]['tokens']
    if len(set(tokens)) != 3:
        raise UnsupportedDevice('Preview-return retained source handles are aliased')
    inputs = source._prefix(historical=True)
    if (record['contract'].get('prefix_sha256') != digest(inputs.event_history)
            or not 0 <= record['write_cursor'] <= len(inputs.writes)):
        raise UnsupportedDevice('Preview-return clock lost its original input history or ACK cursor')
    assertion, acknowledgment = record['assertion_write_index'], record['ack_write_index']
    if record['state'] in ('asserted', 'acknowledged'):
        if type(assertion) is not int or not 0 <= assertion <= record['write_cursor']:
            raise UnsupportedDevice('Preview-return physical assertion lost its input write boundary')
    elif assertion is not None:
        raise UnsupportedDevice('Preview-return waiting input claims an assertion write boundary')
    if record['state'] == 'acknowledged':
        if (type(acknowledgment) is not int or not assertion <= acknowledgment < record['write_cursor']
                or inputs.writes[acknowledgment]['cpu'] != 0
                or inputs.writes[acknowledgment]['address'] != hex(inputs.BASE + 0x388)
                or inputs.writes[acknowledgment]['value'] != '0x1'):
            raise UnsupportedDevice('Preview-return acknowledgment lost the actual CPU0 selector1 write')
    elif acknowledgment is not None:
        raise UnsupportedDevice('Preview-return input claims an acknowledgment without its physical boundary')
    previous = record['due_tick']
    for field in ('asserted_tick', 'acknowledged_tick', 'eoi_tick'):
        value = record[field]
        if value is not None:
            if type(value) is not int or previous is None or not previous <= value <= source.board.virtual_ticks:
                raise UnsupportedDevice('Preview-return clock physical assertion/ACK/EOI chronology changed')
        previous = value
    if ((record['state'] in ('asserted', 'acknowledged')) != (record['asserted_tick'] is not None)
            or (record['state'] == 'acknowledged') != (record['acknowledged_tick'] is not None)
            or record['state'] != 'acknowledged' and record['eoi_tick'] is not None):
        raise UnsupportedDevice('Preview-return clock claims an unobserved physical boundary')
    if record['state'] == 'canceled':
        if (type(record['canceled_tick']) is not int or not record['requested_tick'] <= record['canceled_tick'] <= source.board.virtual_ticks
                or not record['reason']):
            raise UnsupportedDevice('Preview-return clock cancellation chronology changed')
    elif record['canceled_tick'] is not None or record['reason'] is not None:
        raise UnsupportedDevice('Preview-return clock claims a cancellation without a withdrawn owner')
    return record


def tick(source, board):
    record = _marker(source)
    if record is None or record.get('eoi_tick') is not None or record.get('state') == 'canceled' or board.failure is not None:
        return
    try:
        inputs = source._device('InputProcessingControl')
        if record['state'] == 'waiting':
            if board.virtual_ticks < record['due_tick']:
                return
            validate(source)
            _, live = _live(source)
            if live != record['contract']:
                raise UnsupportedDevice('Preview-return caller changed before its one physical clock')
            record.update(state='asserted', asserted_tick=board.virtual_ticks, write_cursor=len(inputs.writes),
                          assertion_write_index=len(inputs.writes))
            _seal(record)
            board.queue_irq(0, IRQ)
            board.record('Post-AF preview-return physical source0 asserted', tick=board.virtual_ticks,
                         limits='No source1, DMA, firmware message, callback or AF result')
        elif record['state'] == 'asserted':
            hits = [(index, item) for index, item in enumerate(inputs.writes[record['write_cursor']:], record['write_cursor'])
                    if item['address'] == hex(inputs.BASE + 0x388) and item['value'] == '0x1']
            if hits:
                validate(source)
                if any(item['cpu'] != 0 for _, item in hits):
                    raise UnsupportedDevice('Preview-return selector1 ACK came from another CPU')
                record.update(state='acknowledged', acknowledged_tick=board.virtual_ticks,
                              write_cursor=len(inputs.writes), ack_write_index=hits[0][0])
                _seal(record)
        if record['state'] == 'acknowledged' and record['eoi_tick'] is None and _drained(source):
            validate(source)
            record['eoi_tick'] = board.virtual_ticks
            _seal(record)
    except UnsupportedDevice as error:
        board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='preview-return-clock')


def due(source):
    record = _marker(source)
    return record['due_tick'] if record is not None and record.get('state') == 'waiting' else None


def summary(source):
    record = validate(source)
    return deepcopy(record)
