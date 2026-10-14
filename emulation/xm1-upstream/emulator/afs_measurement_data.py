"""One fresh scene input for the stock generation4 measurement acquisition.

The complete hardware request latches BankA before a separate post-enable
source8 edge rotates desired registers. Declared consumer bytes precede IRQ46;
the real92C8CC paired ACK,11F8EF8 and12F604C own consumption and software
metadata. No callback, result flag or camera counter is written here.
"""
from copy import deepcopy
import hashlib

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import measurement_scene
from . import afs_statistics_frame as old_statistics
from .afs_scene_fixture import consumer_payloads
from .virtual_source import ram_span


PROFILE='declared-afs-measurement-lens-scene-consumer-statistics-unmeasured'
CONTINUATION_PROFILE='declared-afs-second-measurement-lens-scene-consumer-statistics-unmeasured'
EVENT_LIMIT=4096
ACK_OFFSETS=(0x88c,0x8f4,0x98c,0x9f4)


def _seal(data):
    data['state_sha256']=hp.digest({key:value for key,value in data.items() if key!='state_sha256'})


def anchor(device,frame=8):
    """Read-only closure projection; never call source or full validation.

    Source9's historical guard invokes this getter while validating its own
    records. Full event/driver validation is performed before new admission;
    this projection checks the entire stored seal and terminal hardware words.
    """
    record=device.rearm.get('measurement') if type(device.rearm) is dict else None
    retained=record.get('data_history',[])+[record.get('data')] if type(record) is dict else []
    profile=PROFILE if frame==8 else CONTINUATION_PROFILE if frame==9 else None
    matches=[item for item in retained if type(item) is dict and item.get('profile')==profile]
    data=matches[0] if len(matches)==1 else None
    from . import producer_measurement
    if (type(frame) is not int or frame not in (8,9) or type(data) is not dict
            or data.get('profile')!=profile or data.get('state')!='acknowledged'
            or data.get('state_sha256')!=hp.digest({key:value for key,value in data.items() if key!='state_sha256'})
            or data.get('latch_sha256')!=hp.digest(data['latch'])
            or data['latch']['producer']!=producer_measurement.source_anchor(device)
            or type(data.get('source_epoch')) is not dict
            or (data['source_epoch'].get('generation'),data['source_epoch'].get('frame'))!=(4,frame)
            or type(data.get('ack_channel')) is not int or data['ack_channel']!=2
            or type(data.get('ack_step')) is not int or data['ack_step']!=0
            or data.get('ack_due_tick') is not None
            or data.get('ack_words')!={device.BASE+offset:0 for offset in ACK_OFFSETS}
            or type(data.get('completed_tick')) is not int
            or type(data.get('acknowledged_tick')) is not int
            or not data['completed_tick']<=data['acknowledged_tick']<=device.board.virtual_ticks):
        raise UnsupportedDevice('Measurement next input requires the sealed completed frame8/9 data/paired ACK')
    fields=('profile','state','source_epoch','acknowledged_tick','ack_channel','ack_step',
            'ack_due_tick','ack_words','state_sha256')
    result={key:deepcopy(data[key]) for key in fields}
    result.update(input_sha256=data['latch']['input_sha256'],producer=deepcopy(data['latch']['producer']))
    return result


def _source(device,input_sha256=None,frame=8,*,full=True):
    from .afs_frame_source import MEASUREMENT_FRAME_PROFILE,MEASUREMENT_NEXT_FRAME_PROFILE
    from . import producer_measurement
    if frame not in (8,9):raise UnsupportedDevice('Measurement data input exceeds two bounded pixel lifetimes')
    profile=MEASUREMENT_FRAME_PROFILE if frame==8 else MEASUREMENT_NEXT_FRAME_PROFILE
    records=producer_measurement._source_records(device.board,full=full)
    matches=[record for record in records if record['profile']==profile and record['frame']==frame
             and (input_sha256 is None or old_statistics._input_hash(record)==input_sha256)]
    if len(matches)!=1 or matches[0]['state']=='canceled':
        raise UnsupportedDevice('Measurement data requires its separate post-enable frame8 identity')
    return matches[0]


def _epoch(board,frame=8,*,full=True):
    from .afs_frame_source import epoch_for
    if full:return epoch_for(board,frame)
    from . import producer_measurement
    return producer_measurement._source_epoch(board,frame)


def _route(device,pending=False):
    old_statistics._route(device,pending=pending)


def _arm(device,profile,frame):
    from . import producer_measurement
    record=producer_measurement.validate(device)
    if (type(profile) is not str or profile!=(PROFILE if frame==8 else CONTINUATION_PROFILE) or record is None
            or frame not in (8,9) or record['request'] is None or record['request']['step']!=22):
        raise UnsupportedDevice('Measurement data requires the actual enabled request and a new explicit profile')
    previous=record.get('data');history=record.get('data_history',[])
    if frame==8:
        if previous is not None or history:raise UnsupportedDevice('Measurement first data already exists')
    elif (type(previous) is not dict or previous.get('profile')!=PROFILE or previous.get('state')!='acknowledged'
            or history or record.get('rotation',{}).get('state')!='completed'
            or record['rotation'].get('step')!=9):
        raise UnsupportedDevice('Measurement second data requires completed first input, rotation and paired ACK')
    marker=_source(device,frame=frame);now=device.board.virtual_ticks
    if (marker['state']!='waiting' or not record['request']['enabled_tick']<now<marker['due_tick']
            or marker.get('measurement_producer')!=producer_measurement.source_anchor(device)
            or marker['due_tick']+device.data_ticks+2*device.ack_ticks>=1<<64):
        raise UnsupportedDevice('Measurement data requires an actual post-enable future sensor input')
    _route(device)
    scene,_=old_statistics._scene(device)
    image=bytes(device.board.read_memory(int(scene['address'],16),scene['bytes']))
    geometry={device.BASE+offset:device.registers[device.BASE+offset] for offset in (0x100,0x104,0x108,0x15c)}
    columns=((geometry[device.BASE+0x100]>>12)&15)+1
    rows=((geometry[device.BASE+0x104]>>12)&15)+1
    # CCDE64 is the firmware's signed logical focus estimate, refreshed by
    # A0D8F8 from the channel4 encoder. The initial target is latched once;
    # it remains part of this virtual scene across subsequent lens motion.
    focus_bytes=bytes(device.board.read_memory(0xccde64,2))
    if len(focus_bytes)!=2:raise UnsupportedDevice('Measurement scene requires the actual signed focus estimate')
    focus=int.from_bytes(focus_bytes,'little',signed=True)
    optical=measurement_scene.lens_scene_grid(image,width=scene['width'],height=scene['height'],
        stride_bytes=scene['stride_bytes'],focus_position=focus,target_position=focus if frame==8 else previous['latch']['optical']['target_position'],
        columns=columns,rows=rows)
    grid=optical['grid']
    payload=consumer_payloads(optical['image'],width=scene['width'],height=scene['height'],stride_bytes=scene['stride_bytes'])
    overrides={(device.BASE+0x500,offset):raw for offset,raw in grid['bank_spans']}
    # Opt-in replay: a recorded camera statistics bank replaces every
    # declared consumer window (no synthetic grid). Selected by the host on
    # the producer before arming; the latch records the bank identity.
    recorded=getattr(device,'measurement_statistics_fixture',None)
    if recorded is not None:
        payload=dict(payload,spans=[(register,offset,bytes(raw)) for register,offset,raw in recorded.spans],
                     assumptions='Recorded camera statistics bank '+recorded.sha256+' replayed into the consumer windows; '
                                 'its acquisition mode and exposure differ from this request')
        overrides={}
    plan=[];ranges=[]
    for register,offset,raw in payload['spans']:
        replacement=overrides.get((register,offset))
        if replacement is not None:
            if len(replacement)>len(raw):raise UnsupportedDevice('Measurement contrast grid escapes its consumer window')
            raw=replacement+bytes(len(raw)-len(replacement))
        address=device.registers[register]+offset
        low,high=ram_span(address,len(raw))
        if address&3 or any(low<end and start<high for start,end in ranges):
            raise UnsupportedDevice('Measurement consumer windows must be disjoint aligned DRAM spans')
        ranges.append((low,high));plan.append(dict(register=register,offset=offset,address=address,bytes=len(raw),payload_hex=raw.hex()))
    prior={device.BASE+offset:device.registers[device.BASE+offset] for offset in ACK_OFFSETS}
    if tuple(prior.values())!=((0,1,0,0) if frame==8 else (0,0,0,0)) or any(type(word) is not int for word in prior.values()):
        raise UnsupportedDevice('Measurement data requires the exact actual post-setup ACK latches')
    latch=dict(producer=producer_measurement.source_anchor(device),input_sha256=old_statistics._input_hash(marker),
               scene=scene,geometry=geometry,plan=plan,grid_profile=grid['profile'],
               optical={key:optical[key] for key in ('profile','focus_position','target_position',
                    'pulses_per_blur_pass','blur_passes','source_sha256')},
               assumptions=payload['assumptions']+'; '+grid['assumptions']+
                 '; '+optical['assumptions']+
                 '; initial target pose is latched from actual signed CCDE64; programmed pixel origin/extent, physical optical filtering, gain and DMA byte counts are unmeasured')
    data=dict(profile=profile,state='waiting-source',armed_tick=now,latch=latch,latch_sha256=hp.digest(latch),
              source_epoch=None,due_tick=None,completed_tick=None,ack_channel=0,ack_step=0,
              ack_due_tick=None,acknowledged_tick=None,prior_ack_words=prior,ack_words=dict(prior),events=[])
    _seal(data)
    if frame==9:
        record['data_history']=[deepcopy(previous)]
        record['data_history_sha256']=hp.digest(record['data_history'])
    record['data']=data
    device.board.record('Fresh measurement consumer data explicitly armed',profile=profile,
        spans=len(plan),consumer_bytes=sum(span['bytes'] for span in plan),geometry=geometry,
        limits=latch['assumptions'])
    return deepcopy(data)


def arm(device,profile=PROFILE):
    return _arm(device,profile,8)


def arm_next(device,profile=CONTINUATION_PROFILE):
    return _arm(device,profile,9)


def validate_history(device,record,*,full_source=True):
    history=record.get('data_history',[])
    if (type(history) is not list or len(history)>1
            or history and record.get('data_history_sha256')!=hp.digest(history)
            or not history and 'data_history_sha256' in record):
        raise UnsupportedDevice('Measurement completed first-data archive changed')
    for item in history:
        if type(item) is not dict or item.get('profile')!=PROFILE or item.get('state')!='acknowledged':
            raise UnsupportedDevice('Measurement archive must retain the closed first paired ACK')
        validate(device,{**record,'data':item},full_source=full_source)
    return history


def validate(device,record,*,check_route=False,full_source=True):
    data=record.get('data')
    if data is None:return None
    from . import producer_measurement
    if (type(data) is not dict or data.get('profile') not in (PROFILE,CONTINUATION_PROFILE)
            or data.get('state_sha256')!=hp.digest({key:value for key,value in data.items() if key!='state_sha256'})
            or data.get('latch_sha256')!=hp.digest(data['latch'])
            or data['latch']['producer']!=producer_measurement.source_anchor(device)
            or type(data['armed_tick']) is not int or not record['request']['enabled_tick']<data['armed_tick']<=device.board.virtual_ticks
            or data['state'] not in ('waiting-source','pending','delivered','acknowledged')):
        raise UnsupportedDevice('Measurement data immutable activation/scene/plan or phase changed')
    frame=8 if data['profile']==PROFILE else 9
    marker=_source(device,data['latch']['input_sha256'],frame,full=full_source)
    if not data['armed_tick']<marker['due_tick'] or marker.get('measurement_producer')!=data['latch']['producer']:
        raise UnsupportedDevice('Measurement data lost its post-enable sensor identity')
    observed=_epoch(device.board,frame,full=full_source)
    if data['state']=='waiting-source':
        if data['source_epoch'] is not None or data['due_tick'] is not None:
            raise UnsupportedDevice('Measurement data claims an unobserved input')
    elif (observed is None or data['source_epoch']!=observed or data['due_tick']!=observed['asserted_tick']+device.data_ticks):
        raise UnsupportedDevice('Measurement data completion must follow the actual new sensor epoch')
    delivered=data['state'] in ('delivered','acknowledged')
    if (delivered and (type(data['completed_tick']) is not int or not data['due_tick']<=data['completed_tick']<=device.board.virtual_ticks)
            or not delivered and data['completed_tick'] is not None):
        raise UnsupportedDevice('Measurement consumer bytes precede their physical input/data delay')
    words=dict(data['prior_ack_words']);channel=step=0;due=None;acknowledged=None;previous=data['completed_tick']
    if tuple(words.values())!=((0,1,0,0) if frame==8 else (0,0,0,0)) or type(data['events']) is not list or len(data['events'])>EVENT_LIMIT:
        raise UnsupportedDevice('Measurement paired ACK initial latches/event budget changed')
    for event in data['events']:
        if (not delivered or type(event) is not dict or set(event)!={'operation','cpu','address','word','tick'}
                or channel>=2 or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement ACK event identity/order changed')
        previous=event['tick'];clear=device.BASE+0x88c+channel*0x100;flag=clear+0x68
        kind,address,word=event['operation'],event['address'],event['word']
        if type(word) is not int or word not in (0,1):raise UnsupportedDevice('Measurement ACK WORD domain changed')
        if kind=='response':
            if event['cpu'] is not None or step not in (2,3) or due is None or address!=flag or word!=0 or previous<due:
                raise UnsupportedDevice('Measurement ACK response lacks its actual request')
            words[flag]=0;due=None
        else:
            if type(event['cpu']) is not int or event['cpu']!=0:raise UnsupportedDevice('Measurement ACK CPU owner changed')
            if kind=='write' and step==0 and address==clear and word==0:step=1;words[clear]=0
            elif kind=='write' and step==1 and address==flag and word==1:step=2;words[flag]=1;due=previous+device.ack_ticks
            elif kind=='read' and step==2 and address==clear and word==0:step=3
            elif kind=='read' and step==3 and address==flag and word==words[flag]:
                if word==1:step=2
                else:channel+=1;step=0;acknowledged=previous if channel==2 else None
            else:raise UnsupportedDevice('Measurement paired ACK skips/reorders a real ISR transaction')
    if (data['ack_channel']!=channel or data['ack_step']!=step or data['ack_due_tick']!=due
            or data['acknowledged_tick']!=acknowledged or (data['state']=='acknowledged')!=(channel==2)
            or data['ack_words']!=words):
        raise UnsupportedDevice('Measurement ACK phase/projection differs from real ordered accesses')
    if check_route:_route(device,pending=delivered)
    return data


def access(device,record,operation,core,address,size,value=None):
    data=record.get('data')
    if data is None or address not in tuple(device.BASE+offset for offset in ACK_OFFSETS):return False
    validate(device,record,full_source=False)
    if (data['state']!='delivered' or type(core) is not int or core!=0 or type(size) is not int or size!=4
            or len(data['events'])>=EVENT_LIMIT):raise UnsupportedDevice('Measurement ACK requires delivered data and its CPU0 WORD owner')
    channel,step=data['ack_channel'],data['ack_step'];clear=device.BASE+0x88c+channel*0x100;flag=clear+0x68
    word=device.registers[address] if operation=='read' else value
    if device.registers[address]!=data['ack_words'][address] or type(word) is not int or word not in (0,1):
        raise UnsupportedDevice('Measurement live ACK latch changed')
    if operation=='write' and step==0 and address==clear and word==0:next_step=1
    elif operation=='write' and step==1 and address==flag and word==1:next_step=2
    elif operation=='read' and step==2 and address==clear and word==0:next_step=3
    elif operation=='read' and step==3 and address==flag and word==data['ack_words'][address]:next_step=2 if word else 0
    else:raise UnsupportedDevice('Measurement ACK requires next real paired transaction')
    now=device.board.virtual_ticks
    data['events'].append(dict(operation=operation,cpu=core,address=address,word=word,tick=now));data['ack_step']=next_step
    if operation=='write':
        old=device.registers[address];device.registers[address]=data['ack_words'][address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
        if step==1:data['ack_due_tick']=now+device.ack_ticks
    if operation=='read' and address==flag and word==0:
        data['ack_channel']+=1
        if data['ack_channel']==2:data.update(state='acknowledged',acknowledged_tick=now)
    _seal(data);return True


def tick(device,record,board):
    if board is not device.board:raise UnsupportedDevice('Measurement data tick belongs to another board')
    data=record.get('data')
    if data is None or data.get('state')=='acknowledged':return False
    # These states own no output yet. Audit full ledgers at the real source
    # transition, DMA, ACK response and public/MMIO consuming boundaries,
    # rather than replaying the earlier1700 desired accesses every quantum.
    state=data.get('state');now=board.virtual_ticks
    frame=8 if data.get('profile')==PROFILE else 9
    if state=='waiting-source':
        sources=[item for item in getattr(board,'devices',()) if type(item).__name__=='AfsFrameSource']
        if len(sources)==1:
            request=sources[0].request
            if (type(request) is dict and request.get('frame')==frame
                    and request.get('state')=='waiting'):
                return True
    elif state=='pending' and type(data.get('due_tick')) is int and now<data['due_tick']:
        return True
    elif state=='delivered' and (data.get('ack_due_tick') is None
            or type(data.get('ack_due_tick')) is int and now<data['ack_due_tick']):
        return True
    from . import producer_measurement
    if producer_measurement.validate(device) is not record:
        raise UnsupportedDevice('Measurement timed effect lost its current producer lifetime')
    validate(device,record)
    if data['state']=='waiting-source':
        observed=_epoch(board,frame)
        if observed is not None:
            data.update(state='pending',source_epoch=observed,due_tick=observed['asserted_tick']+device.data_ticks);_seal(data)
    if data['state']=='pending' and board.virtual_ticks>=data['due_tick']:
        validate(device,record,check_route=True)
        scene=data['latch']['scene'];image=bytes(board.read_memory(int(scene['address'],16),scene['bytes']))
        if hashlib.sha256(image).hexdigest()!=scene['payload_sha256']:
            raise UnsupportedDevice('Measurement frozen scene changed before data completion')
        for span in data['latch']['plan']:board.write_memory(span['address'],bytes.fromhex(span['payload_hex']),cpu_id=0)
        data.update(state='delivered',completed_tick=board.virtual_ticks);_seal(data)
        board.queue_irq(0,46)
        board.record('Measurement scene consumer bytes delivered before IRQ46',profile=data['profile'],
            source_epoch=data['source_epoch'],limits=data['latch']['assumptions'])
    elif data['state']=='delivered' and data['ack_due_tick'] is not None and board.virtual_ticks>=data['ack_due_tick']:
        address=device.BASE+0x8f4+data['ack_channel']*0x100
        if device.registers[address]!=1 or data['ack_words'][address]!=1:
            raise UnsupportedDevice('Measurement data ACK response lacks its actual preceding flag store')
        data['events'].append(dict(operation='response',cpu=None,address=address,word=0,tick=board.virtual_ticks))
        device.registers[address]=data['ack_words'][address]=0;data['ack_due_tick']=None;_seal(data)
    return data['state']!='acknowledged'


def due(record):
    data=record.get('data')
    if data is None:return None
    return data['due_tick'] if data['state']=='pending' else data['ack_due_tick'] if data['state']=='delivered' else None
