"""One allocated monochrome Image output after the stock finite request.

Rows are a declared scene law. Stock Source0, public 11F79F8, IRQ44,
92A71C/11F5C40 and the real control-bit stop/retirement own all guest work.
This optional activation child is inactive until explicitly admitted.
"""
from copy import copy,deepcopy
import hashlib

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import afs_still_image as rows

PROFILE='declared-still-allocated-monochrome-image-dma-irq44-unmeasured'
CLOCK_PROFILE='declared-afs-generation9-still-image-finite-source0-clock-unmeasured'
KEY='data'
SCENE_FIELDS={'address','bytes','width','height','stride_bytes','payload_sha256'}
WORDS=((0x15d502c,4,7),(0x15d5091,1,1),(0x15d50b4,4,1),
       (0x15f66a4,4,3),(0x15f66a8,4,0x131db24),(0x15f68a0,4,0x400),
       (0x15f6898,1,0),(0x15f689c,4,0),(0xccc1f0,4,0x11f5c40),(0xccc1f4,4,1))


def owner(device):
    from . import image_still_activation as activation
    parent=activation.owner(device)
    return parent.get(KEY) if type(parent) is dict else None


def historical(device):
    record=owner(device)
    if record is None:return device
    proxy=copy(device);proxy.still_reset=dict(device.still_reset)
    proxy.still_reset['configuration']=dict(device.still_reset['configuration'])
    proxy.still_reset['configuration']['prepare']=dict(device.still_reset['configuration']['prepare'])
    proxy.still_reset['configuration']['prepare']['activation']={k:v for k,v in
        device.still_reset['configuration']['prepare']['activation'].items() if k!=KEY}
    proxy.registers=dict(record['prior_words'])
    return proxy


def activation_anchor(device):
    """Pure selected activation projection; no Source/data/public recursion."""
    from . import image_still_activation as activation
    record=activation.validate(historical(device),history=False)
    if record is None or record['state']!='complete' or len(record['arms'])!=1:
        raise UnsupportedDevice('Still Image output awaits the complete 39-access activation')
    arm=record['arms'][0]
    return dict(profile=record['profile'],completed_tick=record['completed_tick'],
        destination=arm['destination'],handle=arm['handle'],sha256=digest(record))


def _read(device,address,size=4):
    return int.from_bytes(device.board.read_memory(address,size,cpu_id=0),'little')


def _source(device):
    matches=[d for d in device.board.devices if type(d).__name__=='AfsFrameSource']
    if len(matches)!=1:raise UnsupportedDevice('Still Image output requires its unique physical clock owner')
    return matches[0]


def _clock(device):
    source=_source(device)
    getter=getattr(source,'still_image_clock_summary',None)
    marker=getter() if getter is not None else None
    if (type(marker) is not dict or tuple(marker.get(k) for k in
            ('profile','generation','cpu','source','physical_irq','state')) !=
            (CLOCK_PROFILE,9,0,0,'0x27','acknowledged')
            or any(type(marker.get(k)) is not int for k in
                ('asserted_tick','acknowledged_tick','eoi_tick'))
            or not 0 <= marker['asserted_tick'] <= marker['acknowledged_tick'] <=
                marker['eoi_tick'] <= device.board.virtual_ticks):
        raise UnsupportedDevice('Still Image output awaits its real Source0 finite boundary and EOI')
    return deepcopy(marker)


def _allocation(device):
    # 133252C installs four primary1C-byte rows at96000-byte intervals;
    # 13322F4 returns this table/count. This profile owns selected slot0.
    base=_read(device,0x1626cd8);capacity=_read(device,0x1626ce8)
    entries=[[_read(device,0x1626d60+i*28+j*4) for j in range(7)] for i in range(4)]
    if (_read(device,0x1626140,2)!=0 or base!=0x45a785c0
            or capacity<4*0x96000 or [entry[0] for entry in entries]!=[base+i*0x96000 for i in range(4)]
            or entries[0][1]!=2):
        raise UnsupportedDevice('Still Image output lost the actual selected96000-byte pool slot')
    return dict(address=base,bytes=0x96000),dict(base=base,capacity=capacity,slot=0,entries=entries)


def _route(device,*,active=False):
    banks=device.board.interrupts.banks
    if len(banks)!=2:raise UnsupportedDevice('Still Image output requires two architectural IRQ banks')
    route=banks[0]
    if (not route.enabled&(1<<44) or route.config.get(0x400+4*44)!=0x92a71c
            or route.config.get(0x800+4*44)!=10 or not route.source_config&(1<<44)
            or any(b.enabled&(1<<45) or b.pending&(1<<45) or b.active==45 or b.injected==45
                or any(e.get('line')==45 for e in b.active_stack) for b in banks)
            or banks[1].enabled&(1<<44) or banks[1].pending&(1<<44)
            or banks[1].active==44 or banks[1].injected==44
            or any(e.get('line')==44 for e in banks[1].active_stack)):
        raise UnsupportedDevice('Still Image output requires the exact sole CPU0 IRQ44 route')
    if not active and (route.pending&(1<<44) or route.active==44 or route.injected==44
            or any(e.get('line')==44 for e in route.active_stack)):
        raise UnsupportedDevice('Still Image output requires drained IRQ44 before data')


def _seal(record):
    record['state_sha256']=digest({k:v for k,v in record.items() if k!='state_sha256'})


def _live(device):
    if any(_read(device,a,n)!=v for a,n,v in WORDS):
        raise UnsupportedDevice('Still Image output awaits the real mode10 finite client3 request')


def _payload_hash(device,data):
    geometry=data['latch']['geometry']
    return hashlib.sha256(b''.join(bytes(device.board.read_memory(
        geometry['destination']+y*geometry['pitch'],geometry['row_bytes'],cpu_id=0))
        for y in range(geometry['height']))).hexdigest()


def arm(device,profile,*,scene,delay_ticks=1024):
    from . import image_still_activation as activation
    if (profile!=PROFILE or owner(device) is not None or device.board.failure is not None
            or type(delay_ticks) is not int or not 1<=delay_ticks<=400000):
        raise UnsupportedDevice('Still Image output requires explicit one-shot bounded admission')
    parent=activation.validate(device)
    anchor=activation_anchor(device);clock=_clock(device);_route(device)
    _live(device)
    if type(scene) is not dict or not SCENE_FIELDS<=set(scene):
        raise UnsupportedDevice('Still Image output requires its frozen declared metering scene')
    selected={key:deepcopy(scene[key]) for key in SCENE_FIELDS}
    if tuple(selected[k] for k in ('width','height','stride_bytes','bytes'))!=(640,426,1280,545280):
        raise UnsupportedDevice('Still Image output requires the qualified640×426 scene')
    allocation,pool=_allocation(device)
    plan=rows.prepare(device,channel=0,allocation=allocation,scene=selected,profile=rows.PROFILE)
    if (plan['geometry']['destination']!=anchor['destination']
            or plan['geometry']['destination']!=_read(device,0x160e538)
            or tuple(plan['geometry'][k] for k in ('width','height','pitch'))!=(640,426,1280)):
        raise UnsupportedDevice('Still Image output differs from the actual allocated bank0 tuple')
    latch=dict(activation=anchor,clock=clock,pool=pool,scene=selected,
               geometry=plan['geometry'],input_sha256=plan['input_sha256'])
    now=device.board.virtual_ticks
    parent[KEY]=dict(profile=profile,state='pending',armed_tick=now,due_tick=now+delay_ticks,
        delay_ticks=delay_ticks,prior_words=dict(device.registers),latch=latch,latch_sha256=digest(latch),
        completed_tick=None,payload_sha256=None,acknowledged_tick=None,events=[],
        retirement=dict(phase='read',due_tick=None,responded_tick=None,responded_event_count=None,events=[]))
    _seal(parent[KEY]);return summary(device)


def validate(device,*,history=True):
    data=owner(device)
    if data is None:return None
    fields={'profile','state','armed_tick','due_tick','delay_ticks','prior_words','latch','latch_sha256',
        'completed_tick','payload_sha256','acknowledged_tick','events','retirement','state_sha256'}
    if (set(data)!=fields or data.get('profile')!=PROFILE
            or data.get('state') not in ('pending','data-before-irq','acknowledged')
            or data.get('state_sha256')!=digest({k:v for k,v in data.items() if k!='state_sha256'})
            or data.get('latch_sha256')!=digest(data['latch'])
            or set(data['latch'])!={'activation','clock','pool','scene','geometry','input_sha256'}
            or data['latch']['activation']!=activation_anchor(device)
            or any(type(data[k]) is not int for k in ('armed_tick','due_tick','delay_ticks'))
            or not 1<=data['delay_ticks']<=400000
            or not 0<=data['armed_tick']<data['due_tick']==data['armed_tick']+data['delay_ticks']<=device.board.virtual_ticks+data['delay_ticks']):
        raise UnsupportedDevice('Still Image output changed its sealed input/activation/time')
    if (data['state']=='pending' and (data['completed_tick'] is not None or data['payload_sha256'] is not None)
            or data['state']!='pending' and (type(data['completed_tick']) is not int
                or not data['due_tick']<=data['completed_tick']<=device.board.virtual_ticks
                or type(data['payload_sha256']) is not str or len(data['payload_sha256'])!=64)
            or len(data['events'])!=(1 if data['state']=='acknowledged' else 0)
            or not data['events'] and data['acknowledged_tick'] is not None):
        raise UnsupportedDevice('Still Image output lost its finite delivery/ACK lifecycle')
    if history:
        from . import image_still_activation as activation
        activation.validate(historical(device))
        if _clock(device)!=data['latch']['clock']:
            raise UnsupportedDevice('Still Image output changed its retained Source0 boundary')
    words=dict(data['prior_words']);base=device.BASES[0];previous=data['armed_tick']
    if data['events']:
        if len(data['events'])!=1 or tuple(data['events'][0][:4])!=('write',0,base+0xd4,1):
            raise UnsupportedDevice('Still Image output lost the real IRQ44 W1C acknowledgment')
        previous=data['events'][0][4];words[base+0xd4]=1
        if data['acknowledged_tick']!=previous or data['completed_tick'] is None or previous<data['completed_tick']:
            raise UnsupportedDevice('Still Image output acknowledged before data delivery')
    close=data['retirement'];phase='read'
    if (set(close)!={'phase','due_tick','responded_tick','responded_event_count','events'}
            or len(close['events'])>8192 or close['events'] and not data['events']
            or close['due_tick'] is not None and (type(close['due_tick']) is not int
                or close['responded_tick'] is not None)
            or close['responded_tick'] is None and close['responded_event_count'] is not None
            or close['responded_tick'] is not None and (type(close['responded_tick']) is not int
                or type(close['responded_event_count']) is not int
                or not 2<=close['responded_event_count']<=len(close['events'])
                or not data['acknowledged_tick']<=close['responded_tick']<=device.board.virtual_ticks)):
        raise UnsupportedDevice('Still Image output retirement timing/schema changed')
    for index,(op,address,word,tick) in enumerate(close['events']):
        expected=('read',base,0x10001) if phase=='read' else ('write',base,0) if phase=='zero' else (
            'read',base,int(close['responded_event_count'] is not None and index>=close['responded_event_count']))
        if ((op,address,word)!=expected or not previous<=tick<=device.board.virtual_ticks
                or close['responded_tick'] is not None and index>=2 and
                   (tick<close['responded_tick'] if word else tick>close['responded_tick'])):
            raise UnsupportedDevice('Still Image output retirement differs from whole92BB28')
        if phase=='read':phase='zero'
        elif phase=='zero':phase='poll';words[base]=0
        elif word==1:phase='completed';words[base]=1
        previous=tick
    if close['responded_tick'] is not None:words[base]=1
    if close['phase']!=phase or device.registers!=words:
        raise UnsupportedDevice('Still Image output control/ACK projection changed')
    return data


def access(device,operation,core,channel,offset,value=None):
    data=owner(device)
    if data is None:return False
    validate(device,history=False)
    base=device.BASES[0];word=device.registers[device.BASES[channel]+offset] if operation=='read' else value
    if core!=0 or channel!=0:raise UnsupportedDevice('Still Image output permits only its CPU0 bank0 transaction')
    if operation=='read' and offset in device.ALL_CONFIG_MASKS:return True
    now=device.board.virtual_ticks
    if (operation,offset,word)==('write',0xd4,1) and data['state']=='data-before-irq':
        _route(device,active=True)
        if device.board.interrupts.banks[0].active!=44:
            raise UnsupportedDevice('Still Image W1C requires its real active IRQ44')
        if _payload_hash(device,data)!=data['payload_sha256']:
            raise UnsupportedDevice('Still Image row payload changed before actual IRQ44 consumption')
        data['events'].append(('write',0,base+offset,1,now));data.update(state='acknowledged',acknowledged_tick=now)
        device.registers[base+offset]=1;_seal(data);return True
    close=data['retirement'];phase=close['phase']
    expected=('read',0,0x10001) if phase=='read' else ('write',0,0) if phase=='zero' else (
        'read',0,int(close['responded_tick'] is not None)) if phase=='poll' else None
    if data['state']!='acknowledged' or (operation,offset,word)!=expected or len(close['events'])>=8192:
        raise UnsupportedDevice('Still Image output requires its next finite stop/poll transaction')
    close['events'].append((operation,base+offset,word,now))
    if phase=='read':close['phase']='zero'
    elif phase=='zero':close.update(phase='poll',due_tick=now+device.stop_ticks);device.registers[base]=0
    elif word==1:close['phase']='completed'
    _seal(data);return True


def tick(device,board):
    data=owner(device)
    if data is None:return
    if data['state']=='pending' and board.virtual_ticks>=data['due_tick']:
        validate(device);_route(device);_live(device)
        selected=data['latch'];plan=rows.prepare(device,channel=0,
            allocation=selected['geometry']['allocation'],scene=selected['scene'],profile=rows.PROFILE)
        if plan['input_sha256']!=selected['input_sha256']:
            raise UnsupportedDevice('Still Image output scene/tuple changed before DMA')
        for address,payload in plan['rows']:board.write_memory(address,payload,cpu_id=0)
        data.update(state='data-before-irq',completed_tick=board.virtual_ticks,
            payload_sha256=hashlib.sha256(b''.join(payload for _,payload in plan['rows'])).hexdigest())
        _seal(data);board.interrupts.raise_irq(0,44)
    close=data['retirement']
    if close['due_tick'] is not None and board.virtual_ticks>=close['due_tick']:
        validate(device,history=False);device.registers[device.BASES[0]]=1
        close.update(due_tick=None,responded_tick=board.virtual_ticks,
            responded_event_count=len(close['events']));_seal(data)


def due(device):
    data=owner(device)
    if data is None:return None
    return data['due_tick'] if data['state']=='pending' else data['retirement']['due_tick']


def summary(device):
    record=validate(device)
    return deepcopy(record) if record is not None else None
