"""One declared final source1 boundary after the actual still tail task.

The clock supplies physical IRQ29 only. Stock9DF038/1310154 own3F00 and
selector0 ACK; stock1310164 sets the real515C join gate.
There is no image, source0 edge, flash, AF result or guest-memory write.
"""
from copy import deepcopy
from hashlib import sha256

from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-afs-generation9-still-tail-source1-clock-unmeasured'
KEY = 'tail_boundary'
PARENT = 'still_initialization'
IRQ = 0x29
WORDS = ((0x15e32d0,4,12),(0x15d502c,4,7),(0x15d5084,4,2),(0x15d5088,4,2),
         (0x173d308,4,1),(0x15f63a4,4,3),(0x15f68a4,4,3),(0x15f68a8,4,0x12f604c),
         (0x15f6bf4,4,1),(0x15f63a8,4,3),(0x15f6e50,4,1),
         (0x15d4f64,4,0x125ae3c),(0x15d4f68,4,0x125adf8),(0x15d4f6c,4,0x125ad54),
         (0x15d4f74,4,0x125ad20),(0x15d4f78,4,0x125e448),
         (0x15d4fcc,4,0),(0x15d4fd8,4,1),(0x15d4fdc,4,0x17c9b00),
         (0x15d5090,1,1),(0x15d5091,1,1),(0x15d5093,1,0),(0x15d50ac,4,0),(0x15f64a0,4,1),(0x15d5094,4,0),
         (0x15d5098,4,0),(0x15d50e4,4,1),(0x15d515c,4,0),(0x15d50b4,1,1),(0x15f6490,4,1))
DESCRIPTOR_START = 0x15d4f88
DESCRIPTOR_SIZE = 0xa4
TABLE_START = 0x17c9b00
TABLE_SIZE = 10*0x6c
FIELDS = {'profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
          'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'initialization_sha256',
          'input_sha256', 'state_sha256', 'state', 'asserted_tick', 'acknowledged_tick',
          'eoi_tick', 'write_cursor', 'assertion_write_index', 'ack_write_index'}
IDENTITY = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
            'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'initialization_sha256')


def _u(source, address, size=4):
    raw = bytes(source.board.read_memory(address, size, cpu_id=0))
    if len(raw) != size:
        raise UnsupportedDevice('Still tail clock caller RAM is truncated')
    return int.from_bytes(raw, 'little')


def _marker(source):
    parent=source.request.get(PARENT) if type(source.request) is dict else None
    return parent.get('qualifying_edge',{}).get('readout_start',{}).get(KEY) if type(parent) is dict else None


def _old(source):
    parent=source.request.get(PARENT)
    if type(parent) is not dict:raise UnsupportedDevice('Still tail clock lost its first initialization marker')
    parent=parent.get('qualifying_edge',{}).get('readout_start')
    if type(parent) is not dict:raise UnsupportedDevice('Still tail lost its qualifying initialization marker')
    return {key:value for key,value in parent.items() if key!=KEY}


def _seal(record):
    record['state_sha256'] = digest({key: value for key, value in record.items() if key not in ('state_sha256','image_boundary')})


def _drained(source):
    return all(not bank.pending & (1 << IRQ) and bank.active != IRQ and bank.injected != IRQ
               for bank in source.board.interrupts.banks)


def _route(source, inputs):
    bank = source.board.interrupts.banks[0]
    if (inputs._event_route(IRQ) != 0 or not _drained(source)
            or bank.config.get(0x400 + 4 * IRQ) != 0x9df038
            or bank.config.get(0x800 + 4 * IRQ) != 9
            or not bank.source_config & (1 << IRQ)):
        raise UnsupportedDevice('Still tail clock lacks its drained CPU0 source1 route')
    return {'cpu': 0, 'irq': IRQ, 'vector': 0x9df038, 'priority': 9, 'source_type': 1}


def _terminal(source):
    from . import afs_still_readout_clock as previous
    parent=_old(source)
    if (set(parent)!=previous.FIELDS or parent['profile']!=previous.PROFILE
            or parent['state']!='acknowledged' or parent['eoi_tick'] is None
            or parent['state_sha256']!=digest({k:v for k,v in parent.items() if k!='state_sha256'})):
        raise UnsupportedDevice('Still tail requires the qualifying selector0 ACK and EOI')
    return digest(parent)


def _live(source):
    inputs = source._prefix(historical=True)
    if (inputs.event_enable_generation != 9 or inputs.registers.get(0x38c) != 0x10203
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.registers.get(0x70) != 0 or inputs.registers.get(0x154) != 0
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Still tail clock input gates differ from the idle generation9 owner')
    if any(_u(source, address, size) != value for address, size, value in WORDS):
        raise UnsupportedDevice('Still tail clock belongs to another sensor/private capture/lower18 owner')
    source0,source1=0xc6dc58,0xc6dc98
    tokens0=[_u(source,source0+4+4*i) for i in range(2)]
    token1=_u(source,source1+4)
    if (_u(source,source0,1)!=2 or _u(source,source1,1)!=1
            or not all(tokens0) or not token1 or len(set(tokens0+[token1]))!=3
            or token1!=_u(source,0x15d5044)
            or [_u(source,source0+24+4*i) for i in range(4)]!=[0x11f6008,2,0x131dbb4,1]
            or (_u(source,source1+24),_u(source,source1+28))!=(0x1310154,1)):
        raise UnsupportedDevice('Still tail differs from its two numeric/image source0 and sole still source1 subscribers')
    producer=source._device('ProducerControl')
    owner=producer.rearm.get('still_capture') if type(producer.rearm) is dict else None
    if (type(owner) is not dict or owner.get('state',{}).get('phase')!='enabled-awaiting-input'
            or owner.get('data') is not None or owner.get('retirement') is not None):
        raise UnsupportedDevice('Still tail requires its completed fresh initializer and no prior data')
    producer_anchor=producer.still_capture_source_anchor()
    descriptor=bytes(source.board.read_memory(DESCRIPTOR_START,DESCRIPTOR_SIZE,cpu_id=0))
    table=bytes(source.board.read_memory(TABLE_START,TABLE_SIZE,cpu_id=0))
    raw=bytes(source.board.read_memory(0x15f6458,56,cpu_id=0))
    if (len(descriptor)!=DESCRIPTOR_SIZE or len(table)!=TABLE_SIZE or len(raw)!=56
            or _u(source,TABLE_START,1)!=1
            or [_u(source,TABLE_START+4*i) for i in range(1,5)]!=[0x450,0x2b8,0xf0,0xf0]
            or any(table[0x6c:])):
        raise UnsupportedDevice('Still tail request/table differs from the actual one-window owner')
    irq=source.board.interrupts.banks[0]
    if (any(bank.pending&(1<<line) or bank.active==line or bank.injected==line
            for bank in source.board.interrupts.banks for line in (0x24,46))
            or not irq.enabled&(1<<0x24) or not irq.enabled&(1<<46)
            or irq.config.get(0x400+4*0x24)!=0x920c08 or irq.config.get(0x800+4*0x24)!=8
            or irq.config.get(0x400+4*46)!=0x92c8cc or irq.config.get(0x800+4*46)!=13):
        raise UnsupportedDevice('Still tail RAW/numeric routes are not live and drained')
    callbacks=[dict(source=0,tokens=tokens0,callbacks=[[0x11f6008,2],[0x131dbb4,1]]),
               dict(source=1,tokens=[token1],callbacks=[[0x1310154,1]])]
    return inputs,dict(generation=9,mask=0x10203,words=[list(word) for word in WORDS],
        callbacks=callbacks,route=_route(source,inputs),prefix_sha256=digest(inputs.event_history),
        descriptor_sha256=sha256(descriptor).hexdigest(),table_sha256=sha256(table).hexdigest(),
        table_pointer=TABLE_START,table_count=1,producer=producer_anchor,
        raw_descriptor_sha256=sha256(raw).hexdigest())


def arm(source, profile, *, cadence_ticks=1000):
    if (profile != PROFILE or source.board.failure is not None or type(source.request) is not dict
            or _marker(source) is not None or PARENT not in source.request
            or type(cadence_ticks) is not int or not 1 <= cadence_ticks <= 400000):
        raise UnsupportedDevice('Still tail clock requires its named one-edge profile and bounded delay')
    source._validate()
    old_sha = _terminal(source)
    inputs, contract = _live(source)
    now = source.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - cadence_ticks:
        raise UnsupportedDevice('Still tail clock virtual time is outside its integer range')
    record = dict(profile=profile, generation=9, cpu=0, source=1, physical_irq='0x29', ack_selector=0,
                  requested_tick=now, due_tick=now + cadence_ticks, cadence_ticks=cadence_ticks,
                  contract=contract, initialization_sha256=old_sha, state='waiting', asserted_tick=None,
                  acknowledged_tick=None, eoi_tick=None, write_cursor=len(inputs.writes),
                  assertion_write_index=None, ack_write_index=None)
    record['input_sha256'] = digest({key: record[key] for key in IDENTITY})
    _seal(record)
    source.request[PARENT]['qualifying_edge']['readout_start'][KEY] = record
    source.board.record('Stock still tail clock explicitly armed', profile=profile,
                        due_tick=record['due_tick'], limits='One source1 IRQ; no source0, image, result or callback')
    return deepcopy(record)


def validate(source):
    record = _marker(source)
    if record is None:
        return None
    if (type(record) is not dict or not FIELDS<=set(record) or not set(record)<=FIELDS|{'image_boundary'}
            or tuple(record[key] for key in ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
               (PROFILE, 9, 0, 1, '0x29', 0)
            or any(type(record[key]) is not int for key in ('requested_tick', 'due_tick', 'cadence_ticks', 'write_cursor'))
            or not 1 <= record['cadence_ticks'] <= 400000
            or not 0 <= record['requested_tick'] < record['due_tick'] == record['requested_tick'] + record['cadence_ticks'] < 1 << 64
            or record['requested_tick'] > source.board.virtual_ticks
            or record['input_sha256'] != digest({key: record[key] for key in IDENTITY})
            or record['state_sha256'] != digest({key: value for key, value in record.items() if key not in ('state_sha256','image_boundary')})
            or record['initialization_sha256'] != _terminal(source)
            or record['state'] not in ('waiting', 'asserted', 'acknowledged')):
        raise UnsupportedDevice('Still tail clock identity, history, timer or plain ledger changed')
    contract = record['contract']
    if (type(contract) is not dict
            or set(contract) != {'generation','mask','words','callbacks','route','prefix_sha256',
                                  'descriptor_sha256','table_sha256','table_pointer','table_count','producer','raw_descriptor_sha256'}
            or contract['generation'] != 9 or contract['mask'] != 0x10203
            or contract['words'] != [list(word) for word in WORDS]
            or contract['table_pointer'] != TABLE_START or contract['table_count'] != 1
            or any(type(contract[key]) is not str or len(contract[key]) != 64
                   for key in ('descriptor_sha256','table_sha256','raw_descriptor_sha256'))
            or contract['route'] != {'cpu': 0, 'irq': IRQ, 'vector': 0x9df038, 'priority': 9, 'source_type': 1}
            or type(contract['callbacks']) is not list or len(contract['callbacks']) != 2):
        raise UnsupportedDevice('Still tail retained owner/route template changed')
    if (type(contract['producer']) is not dict or set(contract['producer'])!={'enabled_tick','sha256'}
            or type(contract['producer']['enabled_tick']) is not int
            or not 0<=contract['producer']['enabled_tick']<=record['requested_tick']
            or type(contract['producer']['sha256']) is not str or len(contract['producer']['sha256'])!=64):
        raise UnsupportedDevice('Still tail lost its pure numeric activation anchor')
    for number, expected in ((0, [[0x11f6008,2],[0x131dbb4,1]]), (1, [[0x1310154,1]])):
        callbacks = contract['callbacks'][number]
        if (type(callbacks) is not dict or set(callbacks) != {'source', 'tokens', 'callbacks'}
                or callbacks['source'] != number or callbacks['callbacks'] != expected
                or type(callbacks['tokens']) is not list or len(callbacks['tokens']) != len(expected)
                or any(type(token) is not int or not 0 < token < 1 << 32 for token in callbacks['tokens'])):
            raise UnsupportedDevice('Still tail retained subscriber template changed')
    tokens = contract['callbacks'][0]['tokens'] + contract['callbacks'][1]['tokens']
    if len(set(tokens)) != 3:
        raise UnsupportedDevice('Still tail retained source handles are aliased')
    if source._device('ProducerControl').still_capture_source_anchor()!=contract['producer']:
        raise UnsupportedDevice('Still tail numeric activation prefix changed')
    inputs = source._prefix(historical=True)
    if (record['contract'].get('prefix_sha256') != digest(inputs.event_history)
            or not 0 <= record['write_cursor'] <= len(inputs.writes)):
        raise UnsupportedDevice('Still tail clock lost its original input history or ACK cursor')
    assertion, acknowledgment = record['assertion_write_index'], record['ack_write_index']
    if record['state'] in ('asserted', 'acknowledged'):
        if type(assertion) is not int or not 0 <= assertion <= record['write_cursor']:
            raise UnsupportedDevice('Still tail physical assertion lost its input write boundary')
    elif assertion is not None:
        raise UnsupportedDevice('Still tail waiting input claims an assertion write boundary')
    if record['state'] == 'acknowledged':
        if (type(acknowledgment) is not int or not assertion <= acknowledgment < record['write_cursor']
                or inputs.writes[acknowledgment]['cpu'] != 0
                or inputs.writes[acknowledgment]['address'] != hex(inputs.BASE + 0x388)
                or inputs.writes[acknowledgment]['value'] != '0x0'):
            raise UnsupportedDevice('Still tail acknowledgment lost the actual CPU0 selector0 write')
    elif acknowledgment is not None:
        raise UnsupportedDevice('Still tail input claims an acknowledgment without its physical boundary')
    previous = record['due_tick']
    for field in ('asserted_tick', 'acknowledged_tick', 'eoi_tick'):
        value = record[field]
        if value is not None:
            if type(value) is not int or previous is None or not previous <= value <= source.board.virtual_ticks:
                raise UnsupportedDevice('Still tail clock physical assertion/ACK/EOI chronology changed')
        previous = value
    if ((record['state'] in ('asserted', 'acknowledged')) != (record['asserted_tick'] is not None)
            or (record['state'] == 'acknowledged') != (record['acknowledged_tick'] is not None)
            or record['state'] != 'acknowledged' and record['eoi_tick'] is not None):
        raise UnsupportedDevice('Still tail clock claims an unobserved physical boundary')
    if record['state']=='waiting':
        _,live=_live(source)
        if live!=record['contract']:
            raise UnsupportedDevice('Still tail waiting caller or descriptor changed')
    if 'image_boundary' in record:
        from . import afs_still_image_clock
        afs_still_image_clock.validate(source)
    return record


def tick(source, board):
    record = _marker(source)
    if record is not None and 'image_boundary' in record:
        from . import afs_still_image_clock
        afs_still_image_clock.tick(source,board)
        return
    if record is None or record.get('eoi_tick') is not None or board.failure is not None:
        return
    try:
        inputs = source._device('InputProcessingControl')
        if record['state'] == 'waiting':
            if board.virtual_ticks < record['due_tick']:
                return
            validate(source)
            _, live = _live(source)
            if live != record['contract']:
                raise UnsupportedDevice('Still tail caller changed before its one physical clock')
            record.update(state='asserted', asserted_tick=board.virtual_ticks, write_cursor=len(inputs.writes),
                          assertion_write_index=len(inputs.writes))
            _seal(record)
            board.queue_irq(0, IRQ)
            board.record('Stock still tail physical source1 asserted', tick=board.virtual_ticks,
                         limits='No source0, DMA, firmware message, callback or AF result')
        elif record['state'] == 'asserted':
            hits = [(index, item) for index, item in enumerate(inputs.writes[record['write_cursor']:], record['write_cursor'])
                    if item['address'] == hex(inputs.BASE + 0x388) and item['value'] == '0x0']
            if hits:
                validate(source)
                if any(item['cpu'] != 0 for _, item in hits):
                    raise UnsupportedDevice('Still tail selector0 ACK came from another CPU')
                record.update(state='acknowledged', acknowledged_tick=board.virtual_ticks,
                              write_cursor=len(inputs.writes), ack_write_index=hits[0][0])
                _seal(record)
        if record['state'] == 'acknowledged' and record['eoi_tick'] is None and _drained(source):
            validate(source)
            record['eoi_tick'] = board.virtual_ticks
            _seal(record)
    except UnsupportedDevice as error:
        board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='still-tail-clock')


def due(source):
    record = _marker(source)
    if record is not None and 'image_boundary' in record:
        from . import afs_still_image_clock
        return afs_still_image_clock.due(source)
    return record['due_tick'] if record is not None and record.get('state') == 'waiting' else None


def summary(source):
    record = validate(source)
    return deepcopy(record)
