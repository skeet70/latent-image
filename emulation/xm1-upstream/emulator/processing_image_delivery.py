"""One explicit paired postprocessing Image plus Input EOF transaction.

RAW-derived rows use the named processing_image_scene simulation law. Payload
precedes physical44/45, their real stop/ACK closure precedes a declared Input
EOF38, and all guest joins, route retirement and callbacks remain stock ARM.
No selected profile means no result, status, interrupt or output bytes.
"""
from copy import copy, deepcopy
import hashlib

from unicorn.arm_const import UC_ARM_REG_PC
from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import processing_image_scene as scene

PROFILE='declared-postprocessing-paired-image-then-input-eof-unmeasured'
KEY='data'
IRQS=(44,45,38)
CALLBACKS={44:(0xccc1f0,0x11f5c40),45:(0xccc200,0x11f5c40),38:(0xc6ddb4,0x11f5b9c)}
ROUTES={44:(0x92a71c,10),45:(0x92a74c,10),38:(0x920c90,8)}
INPUT_BASE=0xfff10000
LIMITS=scene.LIMITS+' Image44/45 before Input38 EOF and all completion/idle delays are declared.'


def input_for(device):
    if type(device).__name__=='InputProcessingControl':return device
    matches=[d for d in device.board.devices if type(d).__name__=='InputProcessingControl']
    return matches[0] if len(matches)==1 else None


def owner(device):
    d=input_for(device)
    r=getattr(d,'processing_activation',None) if d is not None else None
    return r.get(KEY) if type(r) is dict else None


def image_for(device):
    matches=[d for d in device.board.devices if type(d).__name__=='ImageWorkControl']
    if len(matches)!=1:raise UnsupportedDevice('Processing output requires one paired Image owner')
    return matches[0]


def contains(device,address,size):
    return owner(device) is not None and address in (INPUT_BASE,INPUT_BASE+0x3ac,INPUT_BASE+0x3b4,
        0xfff12000,0xfff13000,0xfff120d4,0xfff130d4)


def _read(device,address):
    return int.from_bytes(device.board.read_memory(address,4,cpu_id=0),'little')


def _seal(r):r['seal']=digest({k:v for k,v in r.items() if k not in ('seal','rows')})


def historical(device):
    """Restore only validated new EOF/result route effects on copied views."""
    r=owner(device)
    if r is None:return device
    board=copy(device.board); views=[copy(d) for d in board.devices];board.devices=views
    for view in views:
        view.board=board
        if type(view).__name__=='InputProcessingControl':
            view.processing_activation={k:v for k,v in view.processing_activation.items() if k!=KEY}
    controller=next(v for v in views if type(v).__name__=='KernelInterrupts')
    controller.banks=[copy(b) for b in device.board.interrupts.banks]
    mask=sum(1<<line for line in IRQS)
    for cpu,bank in enumerate(controller.banks):
        bank.config=dict(bank.config);bank.active_stack=[dict(e) for e in bank.active_stack if e.get('line') not in IRQS]
        bank.pending&=~mask
        if bank.active in IRQS:bank.active=None
        if bank.injected in IRQS:bank.injected=None
        bank.enabled=(bank.enabled&~mask)|(r['route'][cpu]['enabled']&mask)
        bank.source_config=(bank.source_config&~mask)|(r['route'][cpu]['source_config']&mask)
        for key,value in r['route'][cpu]['config'].items():
            if key<0x400:
                field_mask=sum(3<<(line%16*2) for line in IRQS
                    if key in (0x140+line//16*4,0x180+line//16*4))
                bank.config[key]=(bank.config.get(key,0)&~field_mask)|(value&field_mask)
            elif value is None:bank.config.pop(key,None)
            else:bank.config[key]=value
    board.interrupts=controller
    original=device.board.read_memory
    def read_memory(address,size,**kwargs):
        if size==4 and address in r['leases']:return r['leases'][address].to_bytes(4,'little')
        return original(address,size,**kwargs)
    board.read_memory=read_memory
    return views[device.board.devices.index(device)]


def _prior(device):
    from . import input_processing_activation as activation
    device=input_for(device)
    r=activation.summary(historical(device))
    if r is None or r['step']!=15 or r['state']!='enabled-awaiting-input':
        raise UnsupportedDevice('Processing output precedes complete Input and paired Image activation')
    return digest({k:v for k,v in r.items() if k!='caller_block'})


def _routes(device,r,*,drained=False):
    for cpu,bank in enumerate(device.board.interrupts.banks):
        if cpu==1 and (bank.enabled|bank.pending)&sum(1<<line for line in IRQS):
            raise UnsupportedDevice('Processing output excludes a CPU1 completion route')
        if cpu==1 and (bank.active in IRQS or bank.injected in IRQS or any(e.get('line') in IRQS for e in bank.active_stack)):
            raise UnsupportedDevice('Processing output excludes a CPU1 active completion')
        for line in IRQS:
            close=r['input_close'] if line==38 else r['image_close'][line-44]
            vector,priority=ROUTES[line]
            enabled=bool(bank.enabled&(1<<line))
            if cpu==0:
                if enabled:
                    if (bank.config.get(0x400+line*4),bank.config.get(0x800+line*4),bank.source_config>>line&1)!=(vector,priority,1):
                        raise UnsupportedDevice('Processing output altered the real IRQ vector/priority/type')
                    for family in (0x140,0x180):
                        key=family+line//16*4;shift=line%16*2
                        if bank.config.get(key,0)>>shift&3!=r['route'][cpu]['config'][key]>>shift&3:
                            raise UnsupportedDevice('Processing output changed its actual IRQ configuration family')
                elif close['phase']!='completed' or (bank.config.get(0x400+line*4),bank.config.get(0x800+line*4))!=(0x42840,priority):
                    raise UnsupportedDevice('Processing output retired a route before its physical stop response')
            if drained and (bank.pending&(1<<line) or bank.active==line or bank.injected==line
                    or any(e.get('line')==line for e in bank.active_stack)):
                raise UnsupportedDevice('Processing output admission requires drained real routes')


def _allocations(device,image):
    root=_read(device,0x1626cd8);capacity=_read(device,0x1626ce8)
    pool=[[_read(device,0x1626d60+n*28+w*4) for w in range(7)] for n in range(4)]
    secondary=[_read(device,0x1626170+w*4) for w in range(7)]
    if (root!=0x45a785c0 or capacity<4*0x96000
            or [p[0] for p in pool]!=[root+n*0x96000 for n in range(4)]
            or _read(device,0x1626140)&0xffff!=1 or pool[1][1]!=2
            or tuple(pool[1][3:5])!=(640,426)
            or secondary[:2]!=[0x4532a000,0x180000]):
        raise UnsupportedDevice('Processing output requires selected slot1 and the explicit resource4 allocation')
    words=image.processing_configuration['words']
    outputs=tuple(dict(desired={offset:words[base+offset] for offset in image.CONFIG_ORDER},
        allocation=dict(address=pool[1][0],bytes=0x96000) if channel==0 else
                   dict(address=secondary[0],bytes=secondary[1]))
        for channel,base in enumerate(image.BASES))
    geometries=[scene.geometry(o['desired'],o['allocation'],channel=n) for n,o in enumerate(outputs)]
    if [(g['destination'],g['width'],g['height'],g['pitch']) for g in geometries]!=[
            (0x05b16cc0,640,426,1280),(0x0532a000,1024,682,2048)]:
        raise UnsupportedDevice('Processing output changed the observed two destination geometries')
    return outputs,dict(root=root,capacity=capacity,pool=pool,secondary=secondary)


def arm(device,profile=PROFILE,*,delay_ticks=1024,idle_ticks=16):
    from . import input_processing_activation as activation
    if (profile!=PROFILE or owner(device) is not None or device.board.failure is not None
            or type(delay_ticks) is not int or not 1<=delay_ticks<=400000
            or type(idle_ticks) is not int or not 1<=idle_ticks<=1000):
        raise UnsupportedDevice('Processing output requires its explicit bounded one-shot profile')
    parent=activation.validate(device,full=True);image=image_for(device)
    if (parent is None or parent['step']!=15 or any(_read(device,a)!=v for a,v in
            ((0x15f66a4,3),(0x15f66a8,0x131db24),(0x15f68a0,0x2c00),
             (0x15f689c,0),(0x15f6e74,0),(0x1625f5c,9),(0x1625f4c,0)))
            or device.board.read_memory(0x15f6898,1,cpu_id=0)!=b'\0'):
        raise UnsupportedDevice('Processing output awaits the real finite paired2C00 request')
    outputs,pool=_allocations(device,image)
    source_geometry=dict(address=0x4e73bc00,width=4992,height=3300,pitch=9984,
        sample_width=4992,sample_height=3296,row_offset=4,column_offset=0)
    raw=bytes(device.board.read_memory(source_geometry['address'],32947200,cpu_id=0))
    if hashlib.sha256(raw).hexdigest()!=parent['source_sha256']:
        raise UnsupportedDevice('Processing output changed its activated full RAW source')
    prepared=scene.prepare(raw,source_geometry,outputs)
    route=[]
    for bank in device.board.interrupts.banks:
        keys=set(0x400+line*4 for line in IRQS)|set(0x800+line*4 for line in IRQS)|{
            0x140+line//16*4 for line in IRQS}|{0x180+line//16*4 for line in IRQS}
        route.append(dict(enabled=bank.enabled,source_config=bank.source_config,
            config={key:bank.config.get(key,0) if key<0x400 else bank.config.get(key) for key in keys}))
    leases={a:_read(device,a) for a in (0xccc1f0,0xccc1f4,0xccc200,0xccc204,0xc6ddb4,0xc6ddc8,0xc6dda0)}
    now=device.board.virtual_ticks
    r=dict(profile=profile,state='waiting',armed_tick=now,due_tick=now+delay_ticks,
        idle_ticks=idle_ticks,prior_sha256=_prior(device),route=route,leases=leases,
        source_geometry=source_geometry,source_sha256=prepared['source_sha256'],pool=pool,
        outputs=prepared['outputs'],input_sha256=prepared['input_sha256'],rows=tuple(p['rows'] for p in prepared['plans']),
        delivered_tick=None,input_due_tick=None,input_tick=None,
        image_ack=[None,None],image_close=[dict(phase='read',events=[],due_tick=None,
            responded_tick=None,responded_event_count=None) for _ in range(2)],
        input_close=dict(phase='irq-ack',events=[],due_tick=None,responded_tick=None,responded_event_count=None),
        image_words={0xfff12000:0x10001,0xfff13000:0x10001},
        input_words={INPUT_BASE:device.registers[0],INPUT_BASE+0x3ac:0xc003},seal=None)
    _seal(r);_routes(device,r,drained=True);parent[KEY]=r
    return summary(device)


def _payload_hash(device,channel):
    r=owner(device);g=r['outputs'][channel]['geometry']
    return hashlib.sha256(b''.join(bytes(device.board.read_memory(g['destination']+y*g['pitch'],g['row_bytes'],cpu_id=0))
        for y in range(g['height']))).hexdigest()


def validate(device,*,history=False):
    device=input_for(device)
    if device is None:return None
    r=owner(device)
    if r is None:return None
    if (r.get('profile')!=PROFILE or r['seal']!=digest({k:v for k,v in r.items() if k not in ('seal','rows')})
            or r['state'] not in ('waiting','image-delivered','awaiting-input-eof','input-delivered','closed')
            or type(r['armed_tick']) is not int or not 0<=r['armed_tick']<r['due_tick']
            or type(r['idle_ticks']) is not int or not 1<=r['idle_ticks']<=1000
            or len(r['rows'])!=2 or len(r['outputs'])!=2 or len(r['image_ack'])!=2
            or any(len(c['events'])>8192 for c in r['image_close']) or len(r['input_close']['events'])>8192):
        raise UnsupportedDevice('Processing output changed its sealed paired data/EOF lifetime')
    words={0xfff12000:0x10001,0xfff13000:0x10001}
    for channel,close in enumerate(r['image_close']):
        base=0xfff12000+channel*0x1000;phase='read';previous=r['armed_tick']
        if r['image_ack'][channel] is not None:
            if r['delivered_tick'] is None or not r['delivered_tick']<=r['image_ack'][channel]<=device.board.virtual_ticks:
                raise UnsupportedDevice('Processing output acknowledged an Image before data')
            words[base+0xd4]=1;previous=r['image_ack'][channel]
        if (set(close)!={'phase','events','due_tick','responded_tick','responded_event_count'}
                or close['responded_tick'] is None and close['responded_event_count'] is not None
                or close['responded_tick'] is not None and (close['due_tick'] is not None
                    or type(close['responded_event_count']) is not int or not 2<=close['responded_event_count']<=len(close['events']))):
            raise UnsupportedDevice('Processing output changed its Image idle-response boundary')
        for index,(op,address,word,tick_) in enumerate(close['events']):
            expected=('read',base,0x10001) if phase=='read' else ('write',base,0) if phase=='zero' else (
                'read',base,int(close['responded_event_count'] is not None and index>=close['responded_event_count']))
            if ((op,address,word)!=expected or any(a is None for a in r['image_ack'])
                    or not previous<=tick_<=device.board.virtual_ticks
                    or close['responded_tick'] is not None and index>=2 and
                        (tick_<close['responded_tick'] if word else tick_>close['responded_tick'])):
                raise UnsupportedDevice('Processing output lost the whole Image stop chronology')
            previous=tick_;words[address]=word
            if phase=='read':phase='zero'
            elif phase=='zero':phase='poll'
            elif word:phase='completed'
        if phase in ('poll','completed'):
            due=close['events'][1][3]+r['idle_ticks']
            if close['responded_tick'] is None and close['due_tick']!=due or close['responded_tick'] is not None and not due<=close['responded_tick']<=device.board.virtual_ticks:
                raise UnsupportedDevice('Processing output changed the declared Image stop latency')
        elif close['due_tick'] is not None or close['responded_tick'] is not None:
            raise UnsupportedDevice('Processing output scheduled Image idle without a real stop')
        if close['responded_tick'] is not None:words[base]=1
        if close['phase']!=phase:raise UnsupportedDevice('Processing output Image stop phase changed')
    if words!=r['image_words']:raise UnsupportedDevice('Processing output Image physical words changed')
    close=r['input_close'];phase='irq-ack';previous=r['input_tick'] or r['armed_tick']
    words={INPUT_BASE:device.registers[0],INPUT_BASE+0x3ac:0xc003}
    if (set(close)!={'phase','events','due_tick','responded_tick','responded_event_count'}
            or close['responded_tick'] is None and close['responded_event_count'] is not None
            or close['responded_tick'] is not None and (close['due_tick'] is not None
                or type(close['responded_event_count']) is not int or not 4<=close['responded_event_count']<=len(close['events']))):
        raise UnsupportedDevice('Processing output changed its Input idle-response boundary')
    for index,(op,address,word,tick_) in enumerate(close['events']):
        ready=close['responded_event_count'] is not None and index>=close['responded_event_count']
        if phase=='poll' and ready:words[INPUT_BASE]|=2
        global_word=words[INPUT_BASE]
        sequence={'irq-ack':('write',INPUT_BASE+0x3b4,0x10000000,'stop-ack'),
            'stop-ack':('write',INPUT_BASE+0x3b4,0x10000003,'global-read'),
            'global-read':('read',INPUT_BASE,global_word,'global-zero'),
            'global-zero':('write',INPUT_BASE,global_word&~2,'poll'),
            'poll':('read',INPUT_BASE,global_word,'control-read' if ready else 'poll'),
            'control-read':('read',INPUT_BASE+0x3ac,0xc003,'control-disable'),
            'control-disable':('write',INPUT_BASE+0x3ac,3,'completed')}
        expected=sequence.get(phase)
        if (r['input_tick'] is None or expected is None or (op,address,word)!=expected[:3]
                or not previous<=tick_<=device.board.virtual_ticks
                or phase=='poll' and close['responded_tick'] is not None and
                    (tick_<close['responded_tick'] if ready else tick_>close['responded_tick'])):
            raise UnsupportedDevice('Processing output lost whole Input EOF/idle/disable chronology')
        phase=expected[3];previous=tick_
        if op=='write':words[address]=word
    if len(close['events'])>=4:
        due=close['events'][3][3]+r['idle_ticks']
        if close['responded_tick'] is None and close['due_tick']!=due or close['responded_tick'] is not None and not due<=close['responded_tick']<=device.board.virtual_ticks:
            raise UnsupportedDevice('Processing output changed declared Input EOF stop latency')
    elif close['due_tick'] is not None or close['responded_tick'] is not None:
        raise UnsupportedDevice('Processing output scheduled Input idle without a real clear')
    if close['responded_tick'] is not None:words[INPUT_BASE]|=2
    if phase!=close['phase'] or words!=r['input_words']:
        raise UnsupportedDevice('Processing output Input physical words/phase changed')
    _routes(device,r)
    if history:
        if r['prior_sha256']!=_prior(device):
            raise UnsupportedDevice('Processing output changed earlier Input/Image/RAW records')
        if hashlib.sha256(device.board.read_memory(r['source_geometry']['address'],32947200,cpu_id=0)).hexdigest()!=r['source_sha256']:
            raise UnsupportedDevice('Processing output changed the full owned RAW source')
        for channel,rows in enumerate(r['rows']):
            g=r['outputs'][channel]['geometry']
            if (len(rows)!=g['height'] or any(a!=g['destination']+y*g['pitch'] or type(row) is not bytes or len(row)!=g['row_bytes']
                    for y,(a,row) in enumerate(rows)) or hashlib.sha256(b''.join(row for _,row in rows)).hexdigest()!=r['outputs'][channel]['payload_sha256']):
                raise UnsupportedDevice('Processing output changed its immutable prepared rows')
            if r['delivered_tick'] is not None and _payload_hash(device,channel)!=r['outputs'][channel]['payload_sha256']:
                raise UnsupportedDevice('Processing output changed produced owned rows')
    return r


def access(device,operation,cpu,address,size,value=None):
    r=validate(device)
    if r is None or type(cpu) is not int or cpu!=0 or type(size) is not int or size!=4:
        raise UnsupportedDevice('Processing output requires its typed CPU0 WORD transaction')
    now=device.board.virtual_ticks;pc=int(device.board.cpus[0].reg_read(UC_ARM_REG_PC))
    if address in (0xfff120d4,0xfff130d4):
        channel=(address-0xfff120d4)//0x1000
        if operation!='write' or value!=1 or pc!=0x92a70c or r['delivered_tick'] is None or r['image_ack'][channel] is not None:
            raise UnsupportedDevice('Processing output excludes acknowledgment without produced paired data')
        if _payload_hash(device,channel)!=r['outputs'][channel]['payload_sha256']:
            raise UnsupportedDevice('Processing output rows changed before their real Image ACK')
        r['image_ack'][channel]=now;r['image_words'][address]=1
    elif address in (0xfff12000,0xfff13000):
        channel=(address-0xfff12000)//0x1000;close=r['image_close'][channel]
        phase=close['phase'];word=r['image_words'][address] if operation=='read' else value
        expected=('read',0x10001,0x92bb50) if phase=='read' else ('write',0,0x92bb5c) if phase=='zero' else (
            'read',int(close['responded_event_count'] is not None),0x92bb60)
        if any(a is None for a in r['image_ack']) or (operation,word,pc)!=expected or phase=='completed':
            raise UnsupportedDevice('Processing output requires paired real ACKs and whole Image idle stop')
        close['events'].append((operation,address,word,now));r['image_words'][address]=word
        if phase=='read':close['phase']='zero'
        elif phase=='zero':close.update(phase='poll',due_tick=now+r['idle_ticks'])
        elif word:close['phase']='completed'
    else:
        close=r['input_close'];phase=close['phase'];global_word=r['input_words'][INPUT_BASE]
        sequence={'irq-ack':('write',INPUT_BASE+0x3b4,0x10000000,0x920acc,'stop-ack'),
            'stop-ack':('write',INPUT_BASE+0x3b4,0x10000003,0x920acc,'global-read'),
            'global-read':('read',INPUT_BASE,global_word,0x923c94,'global-zero'),
            'global-zero':('write',INPUT_BASE,global_word&~2,0x923c9c,'poll'),
            'poll':('read',INPUT_BASE,global_word,0x920be8,'poll' if not global_word&2 else 'control-read'),
            'control-read':('read',INPUT_BASE+0x3ac,0xc003,0x920b68,'control-disable'),
            'control-disable':('write',INPUT_BASE+0x3ac,3,0x920b98,'completed')}
        expected=sequence.get(phase)
        word=r['input_words'].get(address) if operation=='read' else value
        if r['input_tick'] is None or expected is None or (operation,address,word,pc)!=expected[:4]:
            raise UnsupportedDevice('Processing output requires whole Input EOF ACK/idle/IRQ-disable transaction')
        close['events'].append((operation,address,word,now));close['phase']=expected[4]
        if operation=='write':r['input_words'][address]=word
        if phase=='global-zero':close['due_tick']=now+r['idle_ticks']
        if close['phase']=='completed':r['state']='closed'
    _seal(r)
    return word if operation=='read' else None


def tick(device,board):
    r=owner(device)
    if r is None:return False
    now=board.virtual_ticks;changed=False
    if (r['state']=='waiting' and now>=r['due_tick'] or
            any(c['due_tick'] is not None and now>=c['due_tick'] for c in r['image_close']+[r['input_close']]) or
            r['state']=='awaiting-input-eof' and now>=r['input_due_tick']):
        validate(device)
    if r['state']=='waiting' and now>=r['due_tick']:
        validate(device,history=True)
        for rows in r['rows']:
            for address,row in rows:board.write_memory(address,row,cpu_id=0)
        r.update(state='image-delivered',delivered_tick=now)
        _seal(r)
        for line in (44,45):board.interrupts.raise_irq(0,line)
        changed=True
    for channel,close in enumerate(r['image_close']):
        if close['due_tick'] is not None and now>=close['due_tick']:
            close.update(due_tick=None,responded_tick=now,responded_event_count=len(close['events']))
            r['image_words'][0xfff12000+channel*0x1000]=1;changed=True
    close=r['input_close']
    if close['due_tick'] is not None and now>=close['due_tick']:
        close.update(due_tick=None,responded_tick=now,responded_event_count=len(close['events']))
        r['input_words'][INPUT_BASE]|=2;changed=True
    if r['state']=='image-delivered' and all(c['phase']=='completed' for c in r['image_close']):
        bank=board.interrupts.banks[0]
        if not bank.pending&((1<<44)|(1<<45)) and bank.active not in (44,45) and bank.injected not in (44,45):
            r.update(state='awaiting-input-eof',input_due_tick=now+r['idle_ticks']);changed=True
    if r['state']=='awaiting-input-eof' and now>=r['input_due_tick']:
        r.update(state='input-delivered',input_tick=now);_seal(r)
        board.interrupts.raise_irq(0,38);changed=True
    if changed:_seal(r)
    return changed


def next_event_tick(device,now):
    r=owner(device)
    if r is None or r['state']=='closed':return None
    due=[c['due_tick'] for c in r['image_close']+[r['input_close']] if c['due_tick'] is not None]
    if r['state']=='waiting':due.append(r['due_tick'])
    elif r['state']=='awaiting-input-eof':due.append(r['input_due_tick'])
    elif r['state']=='image-delivered' and all(c['phase']=='completed' for c in r['image_close']):due.append(now+1)
    return min(due) if due else None


def summary(device):
    r=validate(device,history=True)
    return deepcopy({k:v for k,v in r.items() if k not in ('seal','rows')}) if r is not None else None
