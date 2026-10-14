"""Two separately offered Source1 boundaries for the stock still8/9 cleanup.

The optional clocks assert physical IRQ0x29 only. Unchanged ARM owns3F00,
selector0 ACK, the synchronous3F51 reply and subscriber retirement.
Neither clock supplies pixels, a task wake, callback or capture result.
"""
from copy import deepcopy
from hashlib import sha256

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import afs_still_image_clock as previous

PROFILE='declared-afs-generation10-still-cleanup-start-source1-clock-unmeasured'
FINISH_PROFILE='declared-afs-generation10-still-cleanup-finish-source1-clock-unmeasured'
KEY='cleanup_start'
IRQ=0x29
FIELDS=previous.FIELDS
IDENTITY=previous.IDENTITY
COMMON=((0x15e32d0,4,12),(0x173d308,4,1),
        (0x15d4f60,4,0x125ad88),(0x15d5020,1,1),(0x15d50a0,4,0),
        (0x15d50a4,4,0),(0x15d50a8,4,0),(0x15d50b8,4,0),(0x15d50c0,4,1),
        (0x15d50c4,4,0),(0x15d5160,4,0),(0x15d5164,4,0),(0x15d5168,4,0),
        (0x15d5094,4,1),(0x15d5098,4,1),(0x15d50e4,4,1),(0x15d515c,4,1),
        (0x15d5170,4,1),(0x15d5174,4,1),(0x15f63a4,4,3),(0x15f63a8,4,0),
        (0x15f68a4,4,0),(0x15f68a8,4,0),(0x15f6bf4,4,0),(0x15f6e50,4,1),
        (0x15f66a4,4,0),(0x1625f5c,4,2),(0x1625f50,4,1),(0x1626d64,4,6))


def _marker(source):
    parent=previous._marker(source)
    return parent.get(KEY) if type(parent) is dict else None


def _base(record):
    return {k:v for k,v in record.items() if k!='finish'}


def _seal(record):
    record['state_sha256']=digest({k:v for k,v in record.items() if k not in ('state_sha256','finish')})


def _terminal(source):
    parent=previous._marker(source)
    old={k:v for k,v in parent.items() if k!=KEY} if type(parent) is dict else None
    if (type(old) is not dict or set(old)!=previous.FIELDS or old['profile']!=previous.PROFILE
            or old['state']!='acknowledged' or type(old['eoi_tick']) is not int
            or old['eoi_tick']>source.board.virtual_ticks
            or old['state_sha256']!=digest({k:v for k,v in old.items() if k!='state_sha256'})):
        raise UnsupportedDevice('Still cleanup awaits its closed Source0 Image boundary')
    return digest(old)


def _image_data(source):
    from . import afs_still_image_delivery as delivery
    data=delivery.validate(source._device('ImageWorkControl'),history=False)
    if data is None or data['state']!='acknowledged' or data['retirement']['phase']!='completed':
        raise UnsupportedDevice('Still cleanup awaits actual Image pixels/IRQ44 and completed finite retirement')
    return dict(profile=data['profile'],state=data['state'],completed_tick=data['completed_tick'],
        acknowledged_tick=data['acknowledged_tick'],payload_sha256=data['payload_sha256'],
        state_sha256=data['state_sha256'])


def _drained(source):
    return all(not b.pending&(1<<IRQ) and b.active!=IRQ and b.injected!=IRQ
        and all(item.get('line')!=IRQ for item in b.active_stack) for b in source.board.interrupts.banks)


def _words(finish):
    # Upper3F53 may register the real delayed Face stop after the first edge.
    # Keep the original start schema exact; finish seals that timer separately.
    words=tuple(w for w in COMMON if not finish or w[0]!=0x15d50b8)
    return words+((0x15d502c,4,9 if finish else 8),(0x15d5158,4,1 if finish else 0))


def _timer(source):
    handle=previous._u(source,0x15d50b8)
    if handle==0:return None
    slots=[i for i in range(32) if previous._u(source,0xc64ef0+4*i)==handle]
    if len(slots)!=1:raise UnsupportedDevice('Still cleanup lost its unique upper Face-stop timer handle')
    slot=slots[0];words=[previous._u(source,0xc64f74+20*slot+4*i) for i in range(5)]
    bank=source.board.interrupts.banks[0]
    if (previous._u(source,0xc64f70)!=1 or words[0]!=1 or not 1<=words[1]<=301
            or words[2:]!=[300,1,0x130f710] or not bank.enabled&(1<<0x22)
            or bank.config.get(0x400+4*0x22)!=0x91d360
            or bank.config.get(0x800+4*0x22)!=12 or not bank.source_config&(1<<0x22)):
        raise UnsupportedDevice('Still cleanup requires the actual750ms application timer and stock130F710 callback')
    return dict(handle=handle,slot=slot,reload=300,periodic=1,callback=0x130f710,
        route=dict(cpu=0,irq=0x22,vector=0x91d360,priority=12,source_type=1))


def _live(source,finish):
    inputs=source._prefix(historical=True)
    if (inputs.event_enable_generation!=10 or inputs.registers.get(0x38c)!=0x10003
            or inputs.registers.get(0x48)!=1 or inputs.registers.get(0x4c)!=1
            or inputs.registers.get(0x70)!=0 or inputs.registers.get(0x154)!=0
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Still cleanup clock belongs to another generation10 sensor owner')
    if any(previous._u(source,a,n)!=v for a,n,v in _words(finish)):
        raise UnsupportedDevice('Still cleanup awaits its exact private8/9 joins and upper Image receiver')
    token0=previous._u(source,0xc6dc5c);token1=previous._u(source,0xc6dc9c)
    callbacks=[(0xc6dc58,token0,0x11f6008,2),(0xc6dc98,token1,0x1310154,1)]
    if (not token0 or not token1 or token0==token1 or token0!=previous._u(source,0x15f6e5c)
            or token1!=previous._u(source,0x15d5044)
            or token1==previous._marker(source)['contract']['callbacks'][1]['tokens'][0]):
        raise UnsupportedDevice('Still cleanup lost its newly registered Source1 and retained numeric handle')
    if any((previous._u(source,a,1),previous._u(source,a+4),previous._u(source,a+24),previous._u(source,a+28))
           !=(1,t,c,p) for a,t,c,p in callbacks):
        raise UnsupportedDevice('Still cleanup requires its retired numeric and new singleton Source1 subscribers')
    banks=source.board.interrupts.banks;bank=banks[0]
    if (inputs._event_route(IRQ)!=0 or not _drained(source)
            or bank.config.get(0x400+4*IRQ)!=0x9df038 or bank.config.get(0x800+4*IRQ)!=9
            or not bank.source_config&(1<<IRQ)
            or any(b.enabled&(1<<line) or b.pending&(1<<line) or b.active==line or b.injected==line
                or any(e.get('line')==line for e in b.active_stack)
                for b in banks for line in (0x24,44,45,46))):
        raise UnsupportedDevice('Still cleanup requires CPU0 Source1 and fully retired pixel/numeric routes')
    descriptor=bytes(source.board.read_memory(previous.DESCRIPTOR_START,previous.DESCRIPTOR_SIZE,cpu_id=0))
    contract=dict(generation=10,mask=0x10003,phase='finish' if finish else 'start',
        words=[list(w) for w in _words(finish)],callbacks=[[s,t,c,p] for s,t,c,p in callbacks],
        route=dict(cpu=0,irq=IRQ,vector=0x9df038,priority=9,source_type=1),
        prefix_sha256=digest(inputs.event_history),descriptor_sha256=sha256(descriptor).hexdigest(),
        image=_image_data(source))
    if finish:contract['timer']=_timer(source)
    return inputs,contract


def arm(source,profile,*,cadence_ticks=1000):
    finish=profile==FINISH_PROFILE
    if (profile not in (PROFILE,FINISH_PROFILE) or source.board.failure is not None
            or type(cadence_ticks) is not int or not 1<=cadence_ticks<=400000):
        raise UnsupportedDevice('Still cleanup requires a named bounded physical-clock profile')
    source._validate();first=_marker(source)
    if (finish and (first is None or first['state']!='acknowledged' or first['eoi_tick'] is None
                   or 'finish' in first) or not finish and first is not None):
        raise UnsupportedDevice('Still cleanup clock is repeated or precedes the first real ACK/EOI')
    old=_base(first) if finish else None
    anchor=digest(old) if finish else _terminal(source)
    inputs,contract=_live(source,finish);now=source.board.virtual_ticks
    if type(now) is not int or not 0<=now<(1<<64)-cadence_ticks:
        raise UnsupportedDevice('Still cleanup virtual clock is outside its unsigned range')
    record=dict(profile=profile,generation=10,cpu=0,source=1,physical_irq='0x29',ack_selector=0,
        requested_tick=now,due_tick=now+cadence_ticks,cadence_ticks=cadence_ticks,contract=contract,
        initialization_sha256=anchor,state='waiting',asserted_tick=None,acknowledged_tick=None,
        eoi_tick=None,write_cursor=len(inputs.writes),assertion_write_index=None,ack_write_index=None)
    record['input_sha256']=digest({k:record[k] for k in IDENTITY});_seal(record)
    if finish:first['finish']=record
    else:previous._marker(source)[KEY]=record
    source.board.record('Still cleanup physical Source1 explicitly armed',profile=profile,
        due_tick=record['due_tick'],limits='One IRQ only; no task wake, pixels, callback or completed-shot flag')
    return deepcopy(record)


def _validate_record(source,record,*,finish=False):
    profile=FINISH_PROFILE if finish else PROFILE
    fields=set(record) if type(record) is dict else set()
    if (fields not in (FIELDS,FIELDS|{'finish'}) or finish and fields!=FIELDS
            or tuple(record[k] for k in ('profile','generation','cpu','source','physical_irq','ack_selector'))
                !=(profile,10,0,1,'0x29',0)
            or record['state'] not in ('waiting','asserted','acknowledged')
            or record['state_sha256']!=digest({k:v for k,v in record.items() if k not in ('state_sha256','finish')})
            or record['input_sha256']!=digest({k:record[k] for k in IDENTITY})
            or any(type(record[k]) is not int for k in ('requested_tick','due_tick','cadence_ticks','write_cursor'))
            or not 1<=record['cadence_ticks']<=400000
            or not 0<=record['requested_tick']<record['due_tick']==record['requested_tick']+record['cadence_ticks']<1<<64
            or record['requested_tick']>source.board.virtual_ticks):
        raise UnsupportedDevice('Still cleanup lost its sealed plain clock/identity history')
    expected=digest(_base(_marker(source))) if finish else _terminal(source)
    if record['initialization_sha256']!=expected:
        raise UnsupportedDevice('Still cleanup changed its separately validated predecessor clock')
    inputs=source._prefix(historical=True);c=record['contract']
    expected_fields={'generation','mask','phase','words','callbacks','route',
                     'prefix_sha256','descriptor_sha256','image'}|({'timer'} if finish else set())
    if (type(c) is not dict or set(c)!=expected_fields
            or c['generation']!=10 or c['mask']!=0x10003 or c['phase']!=('finish' if finish else 'start')
            or c['words']!=[list(w) for w in _words(finish)]
            or c['route']!=dict(cpu=0,irq=IRQ,vector=0x9df038,priority=9,source_type=1)
            or c['image']!=_image_data(source) or c['prefix_sha256']!=digest(inputs.event_history)
            or type(c['descriptor_sha256']) is not str or len(c['descriptor_sha256'])!=64
            or not 0<=record['write_cursor']<=len(inputs.writes)):
        raise UnsupportedDevice('Still cleanup changed its retained Image/route/subscriber contract')
    timer=c.get('timer')
    if finish and timer is not None and (type(timer) is not dict
            or set(timer)!={'handle','slot','reload','periodic','callback','route'}
            or type(timer['handle']) is not int or not 0<timer['handle']<1<<32
            or type(timer['slot']) is not int or not 0<=timer['slot']<32
            or (timer['reload'],timer['periodic'],timer['callback'])!=(300,1,0x130f710)
            or timer['route']!=dict(cpu=0,irq=0x22,vector=0x91d360,priority=12,source_type=1)):
        raise UnsupportedDevice('Still cleanup retained upper Face-stop timer changed')
    if (type(c['callbacks']) is not list or len(c['callbacks'])!=2
            or any(type(entry) is not list or len(entry)!=4 for entry in c['callbacks'])
            or [(entry[0],entry[2],entry[3]) for entry in c['callbacks']]
               !=[(0xc6dc58,0x11f6008,2),(0xc6dc98,0x1310154,1)]
            or any(type(entry[1]) is not int or not 0<entry[1]<1<<32 for entry in c['callbacks'])
            or c['callbacks'][0][1]==c['callbacks'][1][1]
            or c['callbacks'][1][1]==previous._marker(source)['contract']['callbacks'][1]['tokens'][0]):
        raise UnsupportedDevice('Still cleanup retained subscriber tokens are changed or aliased')
    assertion,ack=record['assertion_write_index'],record['ack_write_index']
    if record['state'] in ('asserted','acknowledged'):
        if type(assertion) is not int or not 0<=assertion<=record['write_cursor']:
            raise UnsupportedDevice('Still cleanup lost its physical assertion write boundary')
    elif assertion is not None:raise UnsupportedDevice('Still cleanup claims a premature physical assertion')
    if record['state']=='acknowledged':
        if (type(ack) is not int or not assertion<=ack<record['write_cursor']
                or inputs.writes[ack]['cpu']!=0 or inputs.writes[ack]['address']!=hex(inputs.BASE+0x388)
                or inputs.writes[ack]['value']!='0x0'):
            raise UnsupportedDevice('Still cleanup lost its actual CPU0 selector0 ACK')
    elif ack is not None:raise UnsupportedDevice('Still cleanup claims a premature selector0 ACK')
    last=record['due_tick']
    for key in ('asserted_tick','acknowledged_tick','eoi_tick'):
        tick=record[key]
        if tick is not None and (type(tick) is not int or last is None or not last<=tick<=source.board.virtual_ticks):
            raise UnsupportedDevice('Still cleanup physical assertion/ACK/EOI chronology changed')
        last=tick
    if ((record['state'] in ('asserted','acknowledged'))!=(record['asserted_tick'] is not None)
            or (record['state']=='acknowledged')!=(record['acknowledged_tick'] is not None)
            or record['state']!='acknowledged' and record['eoi_tick'] is not None):
        raise UnsupportedDevice('Still cleanup claims an unobserved hardware boundary')
    if record['state']=='waiting' and _live(source,finish)[1]!=c:
        raise UnsupportedDevice('Still cleanup caller changed before its physical boundary')


def validate(source):
    record=_marker(source)
    if record is None:return None
    _validate_record(source,record)
    if 'finish' in record:
        if record['state']!='acknowledged' or record['eoi_tick'] is None:
            raise UnsupportedDevice('Still cleanup finish precedes its first physical closure')
        _validate_record(source,record['finish'],finish=True)
    return record


def tick(source,board):
    first=_marker(source)
    if first is None or board.failure is not None:return
    record=first.get('finish',first)
    if record.get('eoi_tick') is not None:return
    try:
        inputs=source._device('InputProcessingControl')
        if record['state']=='waiting':
            if board.virtual_ticks<record['due_tick']:return
            validate(source)
            record.update(state='asserted',asserted_tick=board.virtual_ticks,
                write_cursor=len(inputs.writes),assertion_write_index=len(inputs.writes));_seal(record)
            board.queue_irq(0,IRQ)
        elif record['state']=='asserted':
            hits=[(i,e) for i,e in enumerate(inputs.writes[record['write_cursor']:],record['write_cursor'])
                  if e['address']==hex(inputs.BASE+0x388) and e['value']=='0x0']
            if hits:
                validate(source)
                if any(e['cpu']!=0 for _,e in hits):raise UnsupportedDevice('Still cleanup ACK came from another CPU')
                record.update(state='acknowledged',acknowledged_tick=board.virtual_ticks,
                    write_cursor=len(inputs.writes),ack_write_index=hits[0][0]);_seal(record)
        if record['state']=='acknowledged' and record['eoi_tick'] is None and _drained(source):
            validate(source);record['eoi_tick']=board.virtual_ticks;_seal(record)
    except UnsupportedDevice as error:
        board.failure=dict(message=str(error),cpu=0,device='AfsFrameSource',operation='still-cleanup-clock')


def due(source):
    first=_marker(source)
    if first is None:return None
    record=first.get('finish',first)
    return record['due_tick'] if record['state']=='waiting' else None


def summary(source):
    record=validate(source)
    return deepcopy(record)
