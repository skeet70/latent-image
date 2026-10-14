"""One declared zero consumer-statistics result for the post-arm preview edge.

Only the 22 consumer window offsets/lengths come from SPAN_LAYOUT. No captured
payload bytes are replayed, and these windows are not measured ISP transfer
lengths. The original 22-access request latches its destinations before the real
11F6008 callback rotates desired registers. Physical IRQ46 and actual 92C8CC
paired ACKs own notification/consumption; no guest metadata or callback is set.
"""
import ctypes
from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from .recorded_statistics import SPAN_LAYOUT
from .input_processing_devices import PREVIEW_FRAME_EVENT_PROFILE

PROFILE='declared-zero-post-arm-preview-consumer-statistics-unmeasured'
ACK_OFFSETS=(0x88c,0x8f4,0x98c,0x9f4)
PRIOR_ACK_WORDS=(0,1,0,0)  # Channel0 F4 remains the request's real setup latch.
EVENT_LIMIT=4096
INPUT_FIELDS=('profile','base_profile','generation','ordinal','attempt','frame','cpu','source',
    'physical_irq','ack_selector','requested_tick','armed_tick','due_tick','delay_ticks',
    'history_sha256','producer','register_words')


def _request(device):
    config=device.preview_configuration
    if type(config) is not dict or type(config.get('request')) is not dict:
        raise UnsupportedDevice('Preview statistics require the new producer request')
    return config['request']


def _request_hash(request):
    return hp.digest({k:v for k,v in request.items() if k not in ('rotation','data','close')})


def _input(device):
    inputs=[d for d in device.board.devices if type(d).__name__=='InputProcessingControl']
    if len(inputs)!=1 or inputs[0].board is not device.board:
        raise UnsupportedDevice('Preview statistics require one same-board input source')
    source=inputs[0];marker=source._validate_preview_frame()
    if marker['profile']!=PREVIEW_FRAME_EVENT_PROFILE:
        raise UnsupportedDevice('Preview statistics require the explicitly selected post-arm edge')
    return source,marker


def _plan(device,request):
    words=request['current_words'];ram_bytes=ctypes.sizeof(device.board.ram)
    result=[];ranges=[]
    for register,offset,_unused_capture_offset,size,scope in SPAN_LAYOUT:
        base=words.get(hex(register));address=base+offset if type(base) is int else -1
        physical=address-0x40000000
        if (type(size) is not int or size<1 or address&3 or size&3
                or not 0x40004000<=address<0x60000000 or physical+size>ram_bytes
                or any(physical<end and begin<physical+size for begin,end in ranges)):
            raise UnsupportedDevice('Preview statistics latched consumer spans escape/overlap aligned DRAM')
        ranges.append((physical,physical+size))
        result.append(dict(register=hex(register),offset=offset,address=hex(address),bytes=size,scope=scope))
    if len(result)!=22 or sum(e['bytes'] for e in result)!=5216:
        raise UnsupportedDevice('Preview statistics consumer window layout changed')
    return result


def _route(device,cpu,inflight):
    banks=device.board.interrupts.banks
    if len(banks)!=2 or any(type(b.enabled) is not int or not 0<=b.enabled<1<<128 for b in banks):
        raise UnsupportedDevice('Preview statistics IRQ enable bank type/domain changed')
    if [i for i,b in enumerate(banks) if b.enabled&(1<<46)]!=[cpu]:
        raise UnsupportedDevice('Preview statistics IRQ46 owner is absent/ambiguous/changed')
    bank=banks[cpu]
    if (type(bank.config) is not dict or type(bank.source_config) is not int
            or not 0<=bank.source_config<1<<128 or type(bank.pending) is not int
            or not 0<=bank.pending<1<<128
            or any(v is not None and (type(v) is not int or not 0<=v<128)
                   for v in (bank.active,bank.injected))
            or bank.config.get(0x400+4*46)!=0x92c8cc or bank.config.get(0x800+4*46)!=13
            or (bank.source_config>>46)&1!=1
            or not inflight and (bank.pending&(1<<46) or bank.active==46 or bank.injected==46)):
        raise UnsupportedDevice('Preview statistics differ from the exact stock IRQ46 route')


def arm(device,profile):
    """Select one future zero result before the post-arm input edge asserts."""
    request=_request(device)
    if type(profile) is not str or profile!=PROFILE or request.get('data') is not None:
        raise UnsupportedDevice('Unknown/repeated preview statistics experiment')
    device._validate_preview_request(device.preview_configuration)
    source,marker=_input(device);now=device.board.virtual_ticks
    if (request.get('step')!=22 or request.get('state')!='enabled-awaiting-fresh-input'
            or request.get('cpu')!=0 or marker['state']!='waiting'
            or not request['enabled_tick']<=now<marker['due_tick']
            or any(type(v) is not int or not 1<=v<1<<64 for v in (device.data_ticks,device.ack_ticks))
            or marker['due_tick']+device.data_ticks+2*device.ack_ticks>=1<<64):
        raise UnsupportedDevice('Preview statistics require a complete request and future typed source/delay')
    _route(device,0,False);plan=_plan(device,request)
    prior={hex(device.BASE+o):device.registers[device.BASE+o] for o in ACK_OFFSETS}
    if (tuple(prior.values())!=PRIOR_ACK_WORDS or any(type(v) is not int for v in prior.values())):
        raise UnsupportedDevice('Preview statistics require the four exact post-setup ACK latches')
    data=dict(profile=profile,cpu=0,state='waiting-source',armed_tick=now,
        request_sha256=_request_hash(request),input_sha256=hp.digest({k:marker[k] for k in INPUT_FIELDS}),
        plan=plan,data_ticks=device.data_ticks,ack_ticks=device.ack_ticks,prior_ack_words=prior,
        source_epoch=None,due_tick=None,completed_tick=None,irq_tick=None,
        transfers=0,bytes=0,irqs=0,ack_channel=0,ack_step=0,ack_requested_tick=None,
        ack_due_tick=None,acknowledged_tick=None,events=[])
    request['data']=data
    device.board.record('Preview zero consumer-statistics explicitly armed',profile=profile,
        armed_tick=now,consumer_bytes=5216,spans=22,
        limits='Latched prior destination tuple; zero experiment, no measured ISP or guest metadata/callback')
    return data.copy()


def validate(device,request,*,check_route=True):
    try:
        return _validate(device,request,check_route=check_route)
    except (KeyError,TypeError,ValueError,OverflowError) as error:
        raise UnsupportedDevice('Preview statistics contain malformed plain state') from error


def _validate(device,request,*,check_route):
    """Validate plain state; never recurse into the producer's parent validator."""
    data=request.get('data')
    keys={'profile','cpu','state','armed_tick','request_sha256','input_sha256','plan','data_ticks','ack_ticks',
        'prior_ack_words','source_epoch','due_tick','completed_tick','irq_tick','transfers','bytes','irqs',
        'ack_channel','ack_step','ack_requested_tick','ack_due_tick','acknowledged_tick','events'}
    now=device.board.virtual_ticks
    if (type(data) is not dict or set(data)!=keys or data.get('profile')!=PROFILE
            or type(data.get('cpu')) is not int or data['cpu']!=0 or request.get('cpu')!=0
            or type(now) is not int or not 0<=now<1<<64
            or type(data.get('armed_tick')) is not int or not request['enabled_tick']<=data['armed_tick']<=now
            or data.get('request_sha256')!=_request_hash(request)
            or type(data.get('data_ticks')) is not int or data['data_ticks']!=device.data_ticks
            or type(data.get('ack_ticks')) is not int or data['ack_ticks']!=device.ack_ticks
            or not 1<=data['data_ticks']<1<<64 or not 1<=data['ack_ticks']<1<<64
            or data.get('plan')!=_plan(device,request)
            or data.get('prior_ack_words')!={hex(device.BASE+o):w for o,w in zip(ACK_OFFSETS,PRIOR_ACK_WORDS)}
            or any(type(v) is not int for v in data['prior_ack_words'].values())
            or data.get('state') not in ('waiting-source','pending','delivered','acknowledged')):
        raise UnsupportedDevice('Preview statistics owner/immutable request/plan/delay changed')
    source,marker=_input(device)
    if (marker['state']=='canceled'
            or data['input_sha256']!=hp.digest({k:marker[k] for k in INPUT_FIELDS})
            or not data['armed_tick']<marker['due_tick']):
        raise UnsupportedDevice('Preview statistics post-arm source identity changed')
    if data['state']!='acknowledged' and not source._preview_frame_gates(marker):
        raise UnsupportedDevice('Preview statistics input engine/source/configuration changed before consumption')
    epoch=source.preview_frame_epoch()
    if data['state']=='waiting-source':
        if data['source_epoch'] is not None or data['due_tick'] is not None:
            raise UnsupportedDevice('Preview statistics claim an unobserved source')
    elif (epoch is None or data['source_epoch']!=epoch
            or epoch.get('profile')!=PREVIEW_FRAME_EVENT_PROFILE
            or tuple(epoch.get(k) for k in ('generation','ordinal','attempt','frame','cpu','source'))!=(6,2,11,4,0,0)
            or type(data['due_tick']) is not int or data['due_tick']!=epoch['asserted_tick']+data['data_ticks']
            or not request['enabled_tick']<epoch['asserted_tick']<=now or data['due_tick']>=1<<64):
        raise UnsupportedDevice('Preview statistics require the actual fresh source epoch/due time')
    delivered=data['state'] in ('delivered','acknowledged')
    if (any(type(data[k]) is not int for k in ('transfers','bytes','irqs'))
            or (data['transfers'],data['bytes'],data['irqs'])!=((1,5216,1) if delivered else (0,0,0))
            or delivered and (type(data['completed_tick']) is not int
                or not data['due_tick']<=data['completed_tick']<=now or data['irq_tick']!=data['completed_tick'])
            or not delivered and (data['completed_tick'] is not None or data['irq_tick'] is not None)):
        raise UnsupportedDevice('Preview statistics delivery/IRQ precedes its source or changed count')
    words=dict(data['prior_ack_words']);channel=step=0;requested=due_tick=None;acknowledged=None
    events=data['events'];previous=data['completed_tick']
    if type(events) is not list or len(events)>EVENT_LIMIT or not delivered and events:
        raise UnsupportedDevice('Preview statistics ACK ledger is unavailable/exhausted')
    for index,event in enumerate(events):
        if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                or type(event['step']) is not int or event['step']!=index
                or type(event['tick']) is not int or not previous<=event['tick']<=now
                or type(event['word']) is not int or not 0<=event['word']<1<<32 or channel==2):
            raise UnsupportedDevice('Preview statistics ACK order/typed timestamp changed')
        previous=event['tick'];clear=hex(device.BASE+0x88c+channel*0x100);flag=hex(device.BASE+0x8f4+channel*0x100)
        kind,address,word=event['operation'],event['address'],event['word']
        if kind=='response':
            if event['cpu'] is not None or step not in (2,3) or due_tick is None or address!=flag or word!=0 or previous<due_tick:
                raise UnsupportedDevice('Preview statistics ACK response lacks its actual preceding request')
            words[flag]=0;requested=due_tick=None
        else:
            if type(event['cpu']) is not int or event['cpu']!=data['cpu']:
                raise UnsupportedDevice('Preview statistics ACK owner changed')
            if kind=='write' and step==0 and address==clear and word==0:step=1;words[clear]=0
            elif kind=='write' and step==1 and address==flag and word==1:
                step=2;words[flag]=1;requested=previous;due_tick=previous+data['ack_ticks']
                if due_tick>=1<<64:
                    raise UnsupportedDevice('Preview statistics ACK delay exceeds the virtual clock')
            elif kind=='read' and address==clear and step==2 and word==0:step=3
            elif kind=='read' and address==flag and step==3 and word==words[flag]:
                if word==1:step=2
                else:channel+=1;step=0;acknowledged=previous if channel==2 else None
            else:raise UnsupportedDevice('Preview statistics require the whole paired ISR clear/request/poll transaction')
    if (any(type(data[k]) is not int for k in ('ack_channel','ack_step'))
            or (data['ack_channel'],data['ack_step'],data['ack_requested_tick'],data['ack_due_tick'],data['acknowledged_tick'])
                !=(channel,step,requested,due_tick,acknowledged)
            or (data['state']=='acknowledged')!=(channel==2)
            or any(type(device.registers.get(int(a,16))) is not int or device.registers[int(a,16)]!=w for a,w in words.items())):
        raise UnsupportedDevice('Preview statistics ACK state/live words differ from ordered effects')
    if check_route:_route(device,data['cpu'],delivered)
    return data


def access(device,request,operation,core,address,size,value=None):
    if address not in tuple(device.BASE+o for o in ACK_OFFSETS):return False
    data=validate(device,request)
    if (data['state']!='delivered' or type(core) is not int or core!=data['cpu'] or size!=4
            or type(size) is not int or len(data['events'])>=EVENT_LIMIT):
        raise UnsupportedDevice('Preview statistics ACK requires its delivered owner/WORD width/budget')
    channel,step=data['ack_channel'],data['ack_step'];clear=device.BASE+0x88c+channel*0x100;flag=device.BASE+0x8f4+channel*0x100
    word=device.registers[address] if operation=='read' else value
    if type(word) is not int or not 0<=word<1<<32:
        raise UnsupportedDevice('Preview statistics ACK word changed type/domain')
    if operation=='write' and step==0 and address==clear and word==0:next_step=1
    elif operation=='write' and step==1 and address==flag and word==1:next_step=2
    elif operation=='read' and address==clear and step==2 and word==0:next_step=3
    elif operation=='read' and address==flag and step==3:next_step=2 if word==1 else 0
    else:raise UnsupportedDevice('Preview statistics require next actual paired ACK access')
    now=device.board.virtual_ticks
    if next_step==2 and operation=='write' and now+data['ack_ticks']>=1<<64:
        raise UnsupportedDevice('Preview statistics ACK delay exceeds the virtual clock')
    data['events'].append(dict(operation=operation,address=hex(address),word=word,cpu=core,tick=now,step=len(data['events'])))
    if operation=='write':
        old=device.registers[address];device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    data['ack_step']=next_step
    if next_step==2 and operation=='write':
        data['ack_requested_tick']=now;data['ack_due_tick']=now+data['ack_ticks']
    if operation=='read' and address==flag and step==3 and word==0:
        data['ack_channel']+=1;data['ack_step']=0
        if data['ack_channel']==2:data.update(state='acknowledged',acknowledged_tick=now)
    return True


def tick(device,board):
    config=device.preview_configuration
    if type(config) is not dict or config.get('request') is None:return False
    request=config['request']
    if type(request) is dict and request.get('data') is None:return False
    if type(request) is not dict or board is not device.board:
        board.failure=dict(message='Preview statistics request/board ownership changed',
                           device='ProducerControl',operation='preview-statistics',cpu=0)
        return True
    data=request['data']
    # Finished notification has no further timed effect. Consuming MMIO still
    # reconstructs the complete ledger; passive time is not an IRQ-route audit.
    if (type(data) is dict and data.get('profile')==PROFILE and data.get('state')=='acknowledged'
            and type(data.get('ack_channel')) is int and data['ack_channel']==2
            and type(data.get('ack_step')) is int and data['ack_step']==0):return True
    try:
        # Route enables can be temporarily bank-masked before the future IRQ.
        # Validate the consumed route immediately before any RAM/IRQ/ACK effect.
        data=validate(device,request,check_route=False)
        if data['state']=='waiting-source':
            source,_marker=_input(device);epoch=source.preview_frame_epoch()
            if epoch is None:return True
            data.update(state='pending',source_epoch=epoch,due_tick=epoch['asserted_tick']+data['data_ticks'])
            validate(device,request,check_route=False)
        if data['state']=='pending' and board.virtual_ticks>=data['due_tick']:
            # Full parent causality/config validation precedes every RAM/IRQ effect.
            device._validate_preview_request(device.preview_configuration)
            for span in data['plan']:board.write_memory(int(span['address'],16),bytes(span['bytes']),cpu_id=data['cpu'])
            data.update(state='delivered',completed_tick=board.virtual_ticks,irq_tick=board.virtual_ticks,
                transfers=1,bytes=5216,irqs=1)
            board.queue_irq(data['cpu'],46)
            board.record('Preview zero consumer-statistics delivered',profile=PROFILE,bytes=5216,spans=22,
                source_epoch=data['source_epoch'],irq=46,limits='Declared zeros, no measured ISP or guest metadata/callback')
        elif (data['state']=='delivered' and data['ack_due_tick'] is not None
                and board.virtual_ticks>=data['ack_due_tick']):
            device._validate_preview_request(device.preview_configuration)
            if len(data['events'])>=EVENT_LIMIT:
                raise UnsupportedDevice('Preview statistics ACK ledger is exhausted')
            address=device.BASE+0x8f4+data['ack_channel']*0x100
            data['events'].append(dict(operation='response',address=hex(address),word=0,cpu=None,
                tick=board.virtual_ticks,step=len(data['events'])))
            device.registers[address]=0;data.update(ack_requested_tick=None,ack_due_tick=None)
    except (UnsupportedDevice,KeyError,TypeError,ValueError,OverflowError) as error:
        board.failure=dict(message=str(error),device='ProducerControl',operation='preview-statistics',cpu=0)
    return True


def due(device):
    config=device.preview_configuration
    request=config.get('request') if type(config) is dict else None
    data=request.get('data') if type(request) is dict else None
    if type(data) is not dict:return None
    value=data.get('due_tick') if data.get('state')=='pending' else data.get('ack_due_tick') if data.get('state')=='delivered' else None
    return value if type(value) is int and 0<=value<1<<64 else None
