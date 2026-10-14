"""One declared Source0 boundary after real still RAW/numeric retirement.

The clock supplies physical IRQ27 only. Stock9DF0B8/131DBB4 own4025 and
selector1 ACK. Stock131E6E8 requests11F79F8 finite Image completion.
There is no pixel DMA, Image IRQ, task wake, callback or guest-memory write.
"""
from copy import copy, deepcopy
from hashlib import sha256

from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-afs-generation9-still-image-finite-source0-clock-unmeasured'
KEY = 'image_boundary'
PARENT = 'still_initialization'
IRQ = 0x27
WORDS = ((0x15e32d0,4,12),(0x15d502c,4,7),(0x15d5084,4,2),(0x15d5088,4,2),
         (0x173d308,4,1),(0x15f63a4,4,3),(0x15f63a8,4,0),(0x15f68a4,4,0),
         (0x15f68a8,4,0),(0x15f6bf4,4,0),(0x15f6e50,4,1),
         (0x15d5090,1,1),(0x15d5091,1,1),(0x15d5093,1,0),(0x15d50b4,4,1),
         (0x15d5094,4,1),(0x15d5098,4,1),(0x15d50e4,4,1),(0x15d515c,4,1),(0x15d5170,4,0),
         (0x1625f5c,4,9),(0x1625f4c,4,0),(0x1625f74,4,1),
         (0x15f66a4,4,1),(0x15f66a8,4,0x131db24),(0x15f68a0,4,0x400),
         (0x15f6898,1,0),(0xccc1f0,4,0x11f5c40),(0xccc1f4,4,1))
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
        raise UnsupportedDevice('Still Image clock caller RAM is truncated')
    return int.from_bytes(raw, 'little')


def _marker(source):
    parent=source.request.get(PARENT) if type(source.request) is dict else None
    return parent.get('qualifying_edge',{}).get('readout_start',{}).get('tail_boundary',{}).get(KEY) if type(parent) is dict else None


def _old(source):
    parent=source.request.get(PARENT)
    if type(parent) is not dict:raise UnsupportedDevice('Still Image clock lost its first initialization marker')
    parent=parent.get('qualifying_edge',{}).get('readout_start',{}).get('tail_boundary')
    if type(parent) is not dict:raise UnsupportedDevice('Still Image lost its qualifying initialization marker')
    return {key:value for key,value in parent.items() if key!=KEY}


def _seal(record):
    record['state_sha256'] = digest({key: value for key, value in record.items() if key not in ('state_sha256','cleanup_start')})


def _drained(source):
    return all(not bank.pending & (1 << IRQ) and bank.active != IRQ and bank.injected != IRQ
               and all(item.get('line')!=IRQ for item in bank.active_stack)
               for bank in source.board.interrupts.banks)


def _route(source, inputs):
    bank = source.board.interrupts.banks[0]
    if (inputs._event_route(IRQ) != 0 or not _drained(source)
            or bank.config.get(0x400 + 4 * IRQ) != 0x9df0b8
            or bank.config.get(0x800 + 4 * IRQ) != 10
            or not bank.source_config & (1 << IRQ)):
        raise UnsupportedDevice('Still Image clock lacks its drained CPU0 source0 route')
    return {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}


def _terminal(source):
    from . import afs_still_tail_clock as previous
    parent=_old(source)
    if (set(parent)!=previous.FIELDS or parent['profile']!=previous.PROFILE
            or parent['state']!='acknowledged' or parent['eoi_tick'] is None
            or parent['state_sha256']!=digest({k:v for k,v in parent.items() if k!='state_sha256'})):
        raise UnsupportedDevice('Still Image requires the completed tail selector0 ACK and EOI')
    return digest(parent)


def _raw_anchor(inputs):
    from . import afs_still_readout
    record=afs_still_readout.validate(inputs)
    if record is None or record['state']!='complete':
        raise UnsupportedDevice('Still Image clock requires actual finite RAW closure')
    return {key:deepcopy(record[key]) for key in ('profile','state','geometry','source','rows_sha256',
                'asserted_tick','acknowledged_tick','write_end','seal')}


def _image_anchor(image):
    # A successor data job changes active control/ACK words, while the39
    # initializer remains historical. This projection owns no data behavior.
    from . import image_still_activation as activation
    parent=activation.owner(image)
    if type(parent) is not dict:raise UnsupportedDevice('Still Image clock lacks its initialized Image bank')
    view=image
    if 'data' in parent:
        view=copy(image);view.still_reset=dict(image.still_reset)
        view.still_reset['configuration']=dict(image.still_reset['configuration'])
        view.still_reset['configuration']['prepare']=dict(image.still_reset['configuration']['prepare'])
        view.still_reset['configuration']['prepare']['activation']={k:v for k,v in parent.items() if k!='data'}
        view.registers=dict(parent['data']['prior_words'])
    record=activation.validate(view,history=False)
    if record['state']!='complete' or len(record['arms'])!=1:
        raise UnsupportedDevice('Still Image clock awaits the completed39-access Image activation')
    arm=record['arms'][0]
    return dict(profile=record['profile'],completed_tick=record['completed_tick'],
        destination=arm['destination'],handle=arm['handle'],sha256=digest(record))


def _live(source):
    inputs = source._prefix(historical=True)
    if (inputs.event_enable_generation != 9 or inputs.registers.get(0x38c) != 0x10203
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.registers.get(0x70) != 0 or inputs.registers.get(0x154) != 0
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Still Image clock input gates differ from the idle generation9 owner')
    if any(_u(source, address, size) != value for address, size, value in WORDS):
        raise UnsupportedDevice('Still Image clock belongs to another sensor/private capture/lower18 owner')
    source0,source1=0xc6dc58,0xc6dc98
    tokens0=[_u(source,source0+4+4*i) for i in range(2)]
    token1=_u(source,source1+4)
    if (_u(source,source0,1)!=2 or _u(source,source1,1)!=1
            or not all(tokens0) or not token1 or len(set(tokens0+[token1]))!=3
            or token1!=_u(source,0x15d5044)
            or [_u(source,source0+24+4*i) for i in range(4)]!=[0x11f6008,2,0x131dbb4,1]
            or (_u(source,source1+24),_u(source,source1+28))!=(0x1310154,1)):
        raise UnsupportedDevice('Still Image differs from its two numeric/image Source0 and sole still Source1 subscribers')
    producer=source._device('ProducerControl')
    numeric_anchor=producer.still_statistics_anchor()
    raw_anchor=_raw_anchor(inputs)
    image_anchor=_image_anchor(source._device('ImageWorkControl'))
    if _u(source,0x1625f40)!=tokens0[1]:
        raise UnsupportedDevice('Still Image clock lost its own registered Source0 handle')
    descriptor=bytes(source.board.read_memory(DESCRIPTOR_START,DESCRIPTOR_SIZE,cpu_id=0))
    table=bytes(source.board.read_memory(TABLE_START,TABLE_SIZE,cpu_id=0))
    raw=bytes(source.board.read_memory(0x15f6458,56,cpu_id=0))
    if (len(descriptor)!=DESCRIPTOR_SIZE or len(table)!=TABLE_SIZE or len(raw)!=56
            or _u(source,TABLE_START,1)!=1
            or [_u(source,TABLE_START+4*i) for i in range(1,5)]!=[0x450,0x2b8,0xf0,0xf0]
            or any(table[0x6c:])):
        raise UnsupportedDevice('Still Image request/table differs from the actual one-window owner')
    irq=source.board.interrupts.banks[0]
    if (any(bank.enabled&(1<<line) or bank.pending&(1<<line) or bank.active==line or bank.injected==line
            or any(item.get('line')==line for item in bank.active_stack)
            for bank in source.board.interrupts.banks for line in (0x24,46))
            or not irq.enabled&(1<<44) or irq.config.get(0x400+4*44)!=0x92a71c
            or irq.config.get(0x800+4*44)!=10 or not irq.source_config&(1<<44)
            or any(bank.pending&(1<<44) or bank.active==44 or bank.injected==44
                or any(item.get('line')==44 for item in bank.active_stack)
                for bank in source.board.interrupts.banks)):
        raise UnsupportedDevice('Still Image clock requires retired RAW/numeric and drained active Image routes')
    callbacks=[dict(source=0,tokens=tokens0,callbacks=[[0x11f6008,2],[0x131dbb4,1]]),
               dict(source=1,tokens=[token1],callbacks=[[0x1310154,1]])]
    return inputs,dict(generation=9,mask=0x10203,words=[list(word) for word in WORDS],
        callbacks=callbacks,route=_route(source,inputs),prefix_sha256=digest(inputs.event_history),
        descriptor_sha256=sha256(descriptor).hexdigest(),table_sha256=sha256(table).hexdigest(),
        table_pointer=TABLE_START,table_count=1,numeric=numeric_anchor,raw=raw_anchor,image=image_anchor,
        raw_descriptor_sha256=sha256(raw).hexdigest())


def arm(source, profile, *, cadence_ticks=1000):
    if (profile != PROFILE or source.board.failure is not None or type(source.request) is not dict
            or _marker(source) is not None or PARENT not in source.request
            or type(cadence_ticks) is not int or not 1 <= cadence_ticks <= 400000):
        raise UnsupportedDevice('Still Image clock requires its named one-edge profile and bounded delay')
    source._validate()
    old_sha = _terminal(source)
    inputs, contract = _live(source)
    now = source.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - cadence_ticks:
        raise UnsupportedDevice('Still Image clock virtual time is outside its integer range')
    record = dict(profile=profile, generation=9, cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                  requested_tick=now, due_tick=now + cadence_ticks, cadence_ticks=cadence_ticks,
                  contract=contract, initialization_sha256=old_sha, state='waiting', asserted_tick=None,
                  acknowledged_tick=None, eoi_tick=None, write_cursor=len(inputs.writes),
                  assertion_write_index=None, ack_write_index=None)
    record['input_sha256'] = digest({key: record[key] for key in IDENTITY})
    _seal(record)
    source.request[PARENT]['qualifying_edge']['readout_start']['tail_boundary'][KEY] = record
    source.board.record('Stock still Image clock explicitly armed', profile=profile,
                        due_tick=record['due_tick'], limits='One Source0 IRQ; no pixel DMA, Image IRQ, task wake or callback')
    return deepcopy(record)


def validate(source):
    record = _marker(source)
    if record is None:
        return None
    if (type(record) is not dict or set(record) not in (FIELDS,FIELDS|{'cleanup_start'})
            or tuple(record[key] for key in ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
               (PROFILE, 9, 0, 0, '0x27', 1)
            or any(type(record[key]) is not int for key in ('requested_tick', 'due_tick', 'cadence_ticks', 'write_cursor'))
            or not 1 <= record['cadence_ticks'] <= 400000
            or not 0 <= record['requested_tick'] < record['due_tick'] == record['requested_tick'] + record['cadence_ticks'] < 1 << 64
            or record['requested_tick'] > source.board.virtual_ticks
            or record['input_sha256'] != digest({key: record[key] for key in IDENTITY})
            or record['state_sha256'] != digest({key: value for key, value in record.items() if key not in ('state_sha256','cleanup_start')})
            or record['initialization_sha256'] != _terminal(source)
            or record['state'] not in ('waiting', 'asserted', 'acknowledged')):
        raise UnsupportedDevice('Still Image clock identity, history, timer or plain ledger changed')
    contract = record['contract']
    if (type(contract) is not dict
            or set(contract) != {'generation','mask','words','callbacks','route','prefix_sha256',
                                  'descriptor_sha256','table_sha256','table_pointer','table_count','numeric','raw','image','raw_descriptor_sha256'}
            or contract['generation'] != 9 or contract['mask'] != 0x10203
            or contract['words'] != [list(word) for word in WORDS]
            or contract['table_pointer'] != TABLE_START or contract['table_count'] != 1
            or any(type(contract[key]) is not str or len(contract[key]) != 64
                   for key in ('descriptor_sha256','table_sha256','raw_descriptor_sha256'))
            or contract['route'] != {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}
            or type(contract['callbacks']) is not list or len(contract['callbacks']) != 2):
        raise UnsupportedDevice('Still Image retained owner/route template changed')
    if (contract['numeric']!=source._device('ProducerControl').still_statistics_anchor()
            or contract['raw']!=_raw_anchor(source._device('InputProcessingControl'))
            or contract['image']!=_image_anchor(source._device('ImageWorkControl'))):
        raise UnsupportedDevice('Still Image retained RAW/numeric closure or activation changed')
    for number, expected in ((0, [[0x11f6008,2],[0x131dbb4,1]]), (1, [[0x1310154,1]])):
        callbacks = contract['callbacks'][number]
        if (type(callbacks) is not dict or set(callbacks) != {'source', 'tokens', 'callbacks'}
                or callbacks['source'] != number or callbacks['callbacks'] != expected
                or type(callbacks['tokens']) is not list or len(callbacks['tokens']) != len(expected)
                or any(type(token) is not int or not 0 < token < 1 << 32 for token in callbacks['tokens'])):
            raise UnsupportedDevice('Still Image retained subscriber template changed')
    tokens = contract['callbacks'][0]['tokens'] + contract['callbacks'][1]['tokens']
    if len(set(tokens)) != 3:
        raise UnsupportedDevice('Still Image retained source handles are aliased')
    inputs = source._prefix(historical=True)
    if (record['contract'].get('prefix_sha256') != digest(inputs.event_history)
            or not 0 <= record['write_cursor'] <= len(inputs.writes)):
        raise UnsupportedDevice('Still Image clock lost its original input history or ACK cursor')
    assertion, acknowledgment = record['assertion_write_index'], record['ack_write_index']
    if record['state'] in ('asserted', 'acknowledged'):
        if type(assertion) is not int or not 0 <= assertion <= record['write_cursor']:
            raise UnsupportedDevice('Still Image physical assertion lost its input write boundary')
    elif assertion is not None:
        raise UnsupportedDevice('Still Image waiting input claims an assertion write boundary')
    if record['state'] == 'acknowledged':
        if (type(acknowledgment) is not int or not assertion <= acknowledgment < record['write_cursor']
                or inputs.writes[acknowledgment]['cpu'] != 0
                or inputs.writes[acknowledgment]['address'] != hex(inputs.BASE + 0x388)
                or inputs.writes[acknowledgment]['value'] != '0x1'):
            raise UnsupportedDevice('Still Image acknowledgment lost the actual CPU0 selector1 write')
    elif acknowledgment is not None:
        raise UnsupportedDevice('Still Image input claims an acknowledgment without its physical boundary')
    previous = record['due_tick']
    for field in ('asserted_tick', 'acknowledged_tick', 'eoi_tick'):
        value = record[field]
        if value is not None:
            if type(value) is not int or previous is None or not previous <= value <= source.board.virtual_ticks:
                raise UnsupportedDevice('Still Image clock physical assertion/ACK/EOI chronology changed')
        previous = value
    if ((record['state'] in ('asserted', 'acknowledged')) != (record['asserted_tick'] is not None)
            or (record['state'] == 'acknowledged') != (record['acknowledged_tick'] is not None)
            or record['state'] != 'acknowledged' and record['eoi_tick'] is not None):
        raise UnsupportedDevice('Still Image clock claims an unobserved physical boundary')
    if record['state']=='waiting':
        _,live=_live(source)
        if live!=record['contract']:
            raise UnsupportedDevice('Still Image waiting caller or descriptor changed')
    if 'cleanup_start' in record:
        from . import afs_still_cleanup_clock
        afs_still_cleanup_clock.validate(source)
    return record


def tick(source, board):
    from . import afs_still_cleanup_clock
    afs_still_cleanup_clock.tick(source,board)
    record = _marker(source)
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
                raise UnsupportedDevice('Still Image caller changed before its one physical clock')
            record.update(state='asserted', asserted_tick=board.virtual_ticks, write_cursor=len(inputs.writes),
                          assertion_write_index=len(inputs.writes))
            _seal(record)
            board.queue_irq(0, IRQ)
            board.record('Stock still Image physical source0 asserted', tick=board.virtual_ticks,
                         limits='No DMA, firmware message, callback or AF result')
        elif record['state'] == 'asserted':
            hits = [(index, item) for index, item in enumerate(inputs.writes[record['write_cursor']:], record['write_cursor'])
                    if item['address'] == hex(inputs.BASE + 0x388) and item['value'] == '0x1']
            if hits:
                validate(source)
                if any(item['cpu'] != 0 for _, item in hits):
                    raise UnsupportedDevice('Still Image selector1 ACK came from another CPU')
                record.update(state='acknowledged', acknowledged_tick=board.virtual_ticks,
                              write_cursor=len(inputs.writes), ack_write_index=hits[0][0])
                _seal(record)
        if record['state'] == 'acknowledged' and record['eoi_tick'] is None and _drained(source):
            validate(source)
            record['eoi_tick'] = board.virtual_ticks
            _seal(record)
    except UnsupportedDevice as error:
        board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='still-image-clock')


def due(source):
    from . import afs_still_cleanup_clock
    child=afs_still_cleanup_clock.due(source)
    if child is not None:return child
    record = _marker(source)
    return record['due_tick'] if record is not None and record.get('state') == 'waiting' else None


def summary(source):
    record = validate(source)
    # The separately validated successor owns only later physical clocks.
    # Image delivery retains the exact original Image clock capsule.
    return deepcopy({k:v for k,v in record.items() if k!='cleanup_start'}) if record is not None else None
