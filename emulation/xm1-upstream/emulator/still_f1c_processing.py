"""Explicit F1C upload/result transaction, with stock IRQ50 and CPU readout.

The configuration plans are recovered ARM contracts. The separately selected
mode0 result law is a declared scene surrogate; it is not silicon calibration
math. This helper never writes a guest result buffer or invokes a callback.
"""
from copy import copy, deepcopy
import hashlib
import struct

from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R5, UC_ARM_REG_R9

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import still_f1c_idle as idle, still_f1c_scene as scene

PROFILE = 'declared-still-f1c-processing-irq50-unmeasured'
SUCCESSOR_PROFILE = 'declared-still-f1c-sequential-initialization-unmeasured'
ATTRIBUTE, BASE, IRQ = 'f1c_processing', idle.BASE, idle.IRQ
BLOCKS = dict(config=(1,64), source=(2,20), black=(3,16), sampling=(4,24),
              coordinates=(5,4), grid=(6,20), coefficients=(7,56),
              gain_map=(8,512), correction_map=(9,768))
SUMS = (0x600,0x690,0x7b0,0x840,0x960,0x9f0,0xb10,0xba0,0xcc0,0xd50,0xe60,0xef0)
COUNTS = (0x720,0x8d0,0xa80,0xc30)


def owner(device): return getattr(device, ATTRIBUTE, None)


def _words(device, address, count):
    return struct.unpack('<' + str(count) + 'I', device.board.read_memory(address,count*4,cpu_id=0))


def _parameter_hash(parameters):
    return digest({name:hashlib.sha256(data).hexdigest() for name,data in parameters.items()})


def _seal(job):
    job['seal'] = digest({k:v for k,v in job.items() if k not in ('seal','parameters','plan','scene','history','initialization')})


def idle_view(device):
    """The immutable completed initializer remains a historical owner."""
    view=copy(device)
    view.phase_busy=False
    return view


def _initialization_view(device, job):
    view=idle_view(device)
    if 'initialization' in job: setattr(view,idle.ATTRIBUTE,job['initialization'])
    if job['state'] in ('initializing','awaiting-configuration'):
        view.phase_busy=job['initialization']['state']!='complete'
    return view


def _history_hash(history):
    return digest([{k:entry[k] for k in ('seal','parameters_sha256','plan_sha256','scene_sha256','initialization_sha256')} for entry in history])


def _history_validate(device, job, *, full=False):
    history=job.get('history',[])
    if 'history' not in job: return
    if (type(history) is not list or not 1<=len(history)<=3
            or job['history_sha256']!=_history_hash(history)
            or [j['request'][0] for j in history]!=list(range(len(history)))):
        raise UnsupportedDevice('F1C successor changed its flat bounded closed-mode history')
    if full:
        for old in history:
            if (old['state']!='complete' or old['seal']!=digest({k:v for k,v in old.items() if k not in ('seal','parameters','plan','scene','history','initialization')})
                    or old['parameters_sha256']!=_parameter_hash(old['parameters'])
                    or old['plan_sha256']!=digest(old['plan']) or old['scene_sha256']!=digest(old['scene'])):
                raise UnsupportedDevice('F1C successor changed a sealed prior mode or result/input identity')
            init=old.get('initialization',idle.owner(device))
            view=idle_view(device); setattr(view,idle.ATTRIBUTE,init)
            if old['initialization_sha256']!=digest(idle.validate(view)):
                raise UnsupportedDevice('F1C successor changed a previous reset/ACK history')


def arm_next_initialization(device,profile=SUCCESSOR_PROFILE,*,idle_ticks=16):
    previous=validate(device,full=True)
    core=device.board.cpus[0]
    if (profile!=SUCCESSOR_PROFILE or previous is None or previous['state']!='complete'
            or previous['request'][0]>=3
            or (int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))!=(0x9319ac,0x10cda90)):
        raise UnsupportedDevice('F1C successor requires the actual next10CDA6C initializer after closed mode')
    _route(device,enabled=False,drained=True)
    pointer=int(core.reg_read(UC_ARM_REG_R5)); request=_words(device,pointer,12)
    if request[0]!=previous['request'][0]+1:
        raise UnsupportedDevice('F1C successor requires the next actual sequential11808B4 request')
    history=deepcopy(previous.get('history',[]))
    closed=deepcopy({k:v for k,v in previous.items() if k not in ('history','history_sha256')})
    _seal(closed); history.append(closed)
    view=copy(device); delattr(view,idle.ATTRIBUTE)
    idle.arm(view,idle.PROFILE,idle_ticks=idle_ticks)
    record=dict(profile=PROFILE,state='initializing',successor_profile=SUCCESSOR_PROFILE,
        armed_tick=device.board.virtual_ticks,history=history,history_sha256=_history_hash(history),
        initialization=idle.owner(view),next_request=request,next_request_address=pointer,seal=None)
    _seal(record); setattr(device,ATTRIBUTE,record); device.phase_busy=True
    return summary(device)


def _route(device, *, drained=False, enabled=True):
    banks=device.board.interrupts.banks; bank=banks[0]
    if (bool(bank.enabled & (1<<IRQ)) != enabled
            or enabled and (bank.config.get(0x400+4*IRQ)!=0x10cda34
                or bank.config.get(0x800+4*IRQ)!=13 or bank.source_config & (1<<IRQ))
            or any(b.enabled & (1<<IRQ) for b in banks[1:])
            or drained and any(b.pending & (1<<IRQ) or b.active==IRQ or b.injected==IRQ for b in banks)):
        raise UnsupportedDevice('F1C processing lacks its exact stock IRQ50 route/EOI ownership')


def _parameters(device, request):
    if (len(request)!=13 or request[0] not in range(4) or request[12]!=0x1180674
            or request[10:12]!=(0x1552400,0x1552c40)):
        raise UnsupportedDevice('F1C processing requires the exact11808B4 request/callback')
    result={}
    for name,(index,size) in BLOCKS.items():
        address=request[index]
        physical=address & 0x3fffffff
        if not address or address & 3 or physical+size>0x20000000:
            raise UnsupportedDevice('F1C request parameter block leaves aligned allocated RAM')
        result[name]=bytes(device.board.read_memory(address,size,cpu_id=0))
    return result


def _pair(data, offset=0, bits=13):
    a,b=struct.unpack_from('<2H',data,offset)
    if max(a,b)>=(1<<bits): raise UnsupportedDevice('F1C packed parameter exceeds its recovered field')
    return a|(b<<16)


def configuration_plan(mode, parameters, control=0):
    """All four stock upload branches; no permission to synthesize their result."""
    p=parameters; cfg=struct.unpack('<16I',p['config'])
    shifts=(4,7,8,9,10,11,12,13,14,16,17,20,21,22,24,25)
    masks=(7,1,1,1,1,1,1,1,3,1,1,1,1,3,1,1)
    control=(control & 0xfc0c000f) | sum((v&m)<<s for v,m,s in zip(cfg,masks,shifts))
    source,stride=struct.unpack_from('<2I',p['source'])
    if source & 7 or stride & 3 or not 0<stride<=0x1fff:
        raise UnsupportedDevice('F1C source/stride differs from930118 alignment/range')
    plan=[('read',BASE+4,None),('write',BASE+4,control),
          ('write',BASE+8,source&0x3fffffff),('write',BASE+0x14,stride)]
    plan += [('write',BASE+o,_pair(p['source'],b)) for o,b in ((0x18,8),(0x1c,12),(0x20,16))]
    plan += [('write',BASE+0x40+4*i,_pair(p['black'],4*i,14)) for i in range(4)]
    s=struct.unpack('<6I',p['sampling'])
    plan.append(('write',BASE+0xc0,(s[0]&255)|((s[1]&1)<<8)|((s[2]&7)<<12)|((s[3]&255)<<16)|((s[4]&1)<<24)|((s[5]&7)<<28)))
    if mode in (0,2):
        g=p['grid']; w,h=struct.unpack_from('<2H',g)
        if w>1023 or h>511: raise UnsupportedDevice('F1C cell dimensions exceed930398 fields')
        plan += [('write',BASE+0x28,w|(h<<16)),('write',BASE+0x8c,_pair(g,4,12))]
        if mode==0:
            plan += [('write',BASE+0x90+4*i,int.from_bytes(bytes((g[8+2*i],g[14+2*i],g[9+2*i],g[15+2*i])),'little')) for i in range(3)]
    elif mode==1:
        plan.append(('write',BASE+0x24,_pair(p['coordinates'])))
        table=p['correction_map']
        for plane in range(12):
            for x in range(8):
                for half in range(2):
                    v=int.from_bytes(bytes(table[plane*64+x+8*(half*4+y)] for y in range(4)),'little')
                    plan.append(('write',BASE+0x300+plane*0x40+x*8+half*4,v))
    elif mode==3:
        plan.append(('write',BASE+0x24,_pair(p['coordinates'])))
        c=p['coefficients']
        for off,src in ((0x50,0),(0x54,4),(0x5c,10),(0x60,14)):
            plan.append(('write',BASE+off,_pair(c,src,12)))
        #930490 issues58 between54 and5C; preserve exact instruction order.
        plan.insert(len(plan)-2,('write',BASE+0x58,_limited(struct.unpack_from('<H',c,8)[0],12)))
        a,b=struct.unpack_from('<2H',c,18)
        plan += [('write',BASE+0x64,_limited(a,12)|(_limited(b,8)<<16)),('write',BASE+0x68,c[22])]
        for i in range(6):
            lanes=c[23+4*i:27+4*i]
            if len(lanes)!=4 or max(lanes)>63: raise UnsupportedDevice('F1C mode3 coefficient exceeds U6')
            plan.append(('write',BASE+0x6c+4*i,int.from_bytes(lanes,'little')))
        plan += [('write',BASE+0x84,_pair(c,48,14)),('write',BASE+0x88,c[52]|(c[53]<<8)|(_limited(struct.unpack_from('<H',c,54)[0],10)<<16))]
        table=struct.unpack('<256H',p['gain_map'])
        for plane in range(4):
            for x in range(8):
                for pair in range(4):
                    a,b=table[plane*64+x+pair*16],table[plane*64+x+pair*16+8]
                    plan.append(('write',BASE+0x100+plane*0x80+x*16+pair*4,_limited(a,11)|(_limited(b,11)<<16)))
    else: raise UnsupportedDevice('F1C mode must be0..3')
    plan.append(('write',BASE+0xcc,1))
    return tuple(plan)


def _limited(value,bits):
    if not 0<=value<(1<<bits): raise UnsupportedDevice('F1C coefficient exceeds its recovered bit field')
    return value


def _metadata(parameters, capacity):
    address,stride,x0,y0,x1,y1,cx,cy=struct.unpack('<2I6H',parameters['source'])
    if (type(capacity) is not int or capacity<=0 or capacity>0x4000000 or not 0<stride<=0x1fff or capacity%(stride*2)
            or not 0<=x0<=x1<stride or not 0<=y0<=y1<capacity//(stride*2)):
        raise UnsupportedDevice('F1C requires an independently owned pitched U16 source capacity/ROI')
    return dict(address=address,capacity=capacity,stride_samples=stride,
        x0=x0,y0=y0,x1=x1,y1=y1,center_x=cx,center_y=cy,packing='u16le-mono12')


def arm(device, profile=PROFILE, *, source_capacity, source_sha256, completion_ticks=1000, idle_ticks=16):
    old=owner(device)
    if (profile!=PROFILE or old is not None and old.get('state')!='awaiting-configuration' or device.board.failure is not None
            or any(type(v) is not int or not 1<=v<=400000 for v in (completion_ticks,idle_ticks))):
        raise UnsupportedDevice('F1C active processing requires its explicit fresh bounded profile')
    prior=idle.validate(_initialization_view(device,old)) if old is not None else idle.validate(device)
    if old is not None: validate(device,full=True)
    if prior is None or prior['state']!='complete':
        raise UnsupportedDevice('F1C configuration requires the complete earlier initialization')
    from .still_processing_devices import PROFILE as parent_profile
    if device.profile!=parent_profile or device.state!='idle' or device.request is not None:
        raise UnsupportedDevice('F1C active job cannot coexist with F0 pixel ownership')
    core=device.board.cpus[0]
    if (int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))!=(0x930084,0x10cdb34):
        raise UnsupportedDevice('F1C active admission requires the real930078/10CDB20 configuration boundary')
    request=_words(device,0x1566708,13); parameters=_parameters(device,request)
    if old is not None and request[:12]!=old['next_request']:
        raise UnsupportedDevice('F1C successor copied request differs from its actual next initializer')
    if _words(device,0x173d9f4,13)!=request or _words(device,0x173d9f0,1)!=(1,):
        raise UnsupportedDevice('F1C requires the actual registered copied request')
    _route(device,drained=True)
    metadata=_metadata(parameters,source_capacity)
    raw=bytes(device.board.read_memory(metadata['address'],metadata['capacity'],cpu_id=0))
    if type(source_sha256) is not str or hashlib.sha256(raw).hexdigest()!=source_sha256:
        raise UnsupportedDevice('F1C owned input SHA differs before configuration')
    plan=configuration_plan(request[0],parameters)
    job=dict(profile=PROFILE,state='configuring',request=request,parameters=parameters,
        parameters_sha256=_parameter_hash(parameters),metadata=metadata,source_sha256=source_sha256,
        initialization_sha256=digest(prior),armed_tick=device.board.virtual_ticks,
        completion_ticks=completion_ticks,idle_ticks=idle_ticks,plan=plan,plan_sha256=digest(plan),
        step=0,registers={BASE:1,BASE+4:0,BASE+0xc8:0,BASE+0xcc:0},scene=None,scene_sha256=None,
        started_tick=None,due_tick=None,result_tick=None,isr_reset=None,task_reset=None,
        result_step=0,closed_tick=None,seal=None)
    if old is not None:
        job.update(history=old['history'],history_sha256=old['history_sha256'],initialization=old['initialization'])
    _seal(job); setattr(device,ATTRIBUTE,job); device.phase_busy=True
    return summary(device)


def arm_scene(device, *, profile):
    job=validate(device,full=True)
    if job is None or job['state']!='configured' or job['scene'] is not None:
        raise UnsupportedDevice('F1C scene requires the actual complete upload before start')
    raw=bytes(device.board.read_memory(job['metadata']['address'],job['metadata']['capacity'],cpu_id=0))
    try:
        result=scene.prepare(raw,source_metadata=job['metadata'],parameters=job['parameters'],mode=job['request'][0],profile=profile)
    except ValueError as exc:
        raise UnsupportedDevice('F1C declared scene remains unqualified: '+str(exc)) from exc
    expected=_readout_addresses(job['request'][0])
    if set(result)!= {'words','identity'} or set(result['words'])!=set(expected):
        raise UnsupportedDevice('F1C scene result does not cover its exact recovered register banks')
    if any(type(v) is not int or not 0<=v<(1<<(14 if any(BASE+o<=a<BASE+o+144 for o in COUNTS) else 26)) for a,v in result['words'].items()):
        raise UnsupportedDevice('F1C scene result leaves the recovered numeric widths')
    job['scene']=deepcopy(result); job['scene_sha256']=digest(result); _seal(job)
    if profile==scene.GAIN_PROFILE:
        job.update(source_span={k:job['metadata'][k] for k in ('address','capacity')},
            output_sha256=result['identity']['output_sha256'],output_tick=None)
        #11808B4 retains its frame in R9 across the real configuration/start
        #calls. Only a matching live RAW descriptor admits a prospective CPU
        #border plan; isolated callers without that frame retain old behavior.
        frame=int(device.board.cpus[0].reg_read(UC_ARM_REG_R9))
        if frame and not frame & 3 and (frame & 0x3fffffff)+0x49c<=0x20000000:
            descriptor=_words(device,frame+0x88,7)
            border=_words(device,frame+0x498,1)[0]
            if (descriptor[1]==job['metadata']['address'] and descriptor[3]==2*job['metadata']['stride_samples']
                    and border>0):
                try:
                    effect=scene.prepare_gain(raw,source_metadata=job['metadata'],parameters=job['parameters'],profile=profile)
                    plan=scene.gain.prepare_cpu_border(effect['output'],descriptor=descriptor,border=border)['identity']
                except ValueError as exc:
                    raise UnsupportedDevice('F1C prospective CPU border remains unqualified: '+str(exc)) from exc
                job['cpu_border']=dict(plan,frame_address=frame,accepted_tick=None)
        _seal(job)
    return summary(device)


def _readout_addresses(mode):
    return tuple(BASE+off+4*i for off in (SUMS if mode==0 else SUMS[:8] if mode==2 else ()) for i in range(36)) + tuple(BASE+off+4*i for off in COUNTS for i in range(36)) if mode in (0,2) else ()


def _expected_output(job):
    mode=job['request'][0]; words=job['scene']['words']; output=bytearray()
    for off in SUMS if mode==0 else SUMS[:8]:
        for y in range(6):
            for x in range(6):
                v=words[BASE+off+4*(6*x+y)]
                if mode==0 and v&(1<<25): v|=0xfc000000
                output+=struct.pack('<I',v)
    for off in COUNTS:
        for y in range(6):
            for x in range(6): output+=struct.pack('<H',words[BASE+off+4*(6*x+y)])
    return bytes(output)


def validate(device, *, full=False):
    job=owner(device)
    if job is None: return None
    _history_validate(device,job,full=full)
    if job.get('state') in ('initializing','awaiting-configuration'):
        if (job.get('seal')!=digest({k:v for k,v in job.items() if k not in ('seal','parameters','plan','scene','history','initialization')})
                or job.get('successor_profile')!=SUCCESSOR_PROFILE
                or job['next_request'][0]!=len(job['history'])):
            raise UnsupportedDevice('F1C successor changed its qualified next initialization')
        idle.validate(_initialization_view(device,job))
        if full and _words(device,job['next_request_address'],12)!=job['next_request']:
            raise UnsupportedDevice('F1C successor caller request changed before configuration')
        return job
    if (type(job) is not dict or job.get('profile')!=PROFILE
            or job.get('state') not in ('configuring','configured','running','irq-issued','reset-isr_reset',
                'isr-closed','reset-task_reset','task-closed','result-read','complete')
            or job.get('seal')!=digest({k:v for k,v in job.items() if k not in ('seal','parameters','plan','scene','history','initialization')})
            or job['parameters_sha256']!=_parameter_hash(job['parameters']) or job['plan_sha256']!=digest(job['plan'])
            or job['initialization_sha256']!=digest(idle.validate(_initialization_view(device,job)))
            or getattr(device,'phase_busy',None) is not (job['state']!='complete')
            or type(job['step']) is not int or not 0<=job['step']<=len(job['plan'])
            or job['scene'] is not None and job['scene_sha256']!=digest(job['scene'])):
        raise UnsupportedDevice('F1C processing changed its sealed request/configuration/result lifecycle')
    if full:
        if job['state']!='complete' and (_words(device,0x1566708,13)!=job['request'] or _parameters(device,job['request'])!=job['parameters']):
            raise UnsupportedDevice('F1C captured request/parameter bytes changed')
        if job['plan']!=configuration_plan(job['request'][0],job['parameters']):
            raise UnsupportedDevice('F1C configuration differs from the whole stock driver plan')
        if 'output_sha256' in job:
            if (job['request'][0]!=3 or job['scene']['identity']['profile']!=scene.GAIN_PROFILE
                    or job['output_sha256']!=job['scene']['identity']['output_sha256']
                    or job['source_span']!={k:job['metadata'][k] for k in ('address','capacity')}
                    or job['output_tick']!=job['result_tick']):
                raise UnsupportedDevice('F1C gain changed its declared output ownership or effect chronology')
            raw=device.board.read_memory(job['metadata']['address'],job['metadata']['capacity'],cpu_id=0)
            expected=job['source_sha256'] if job['output_tick'] is None else job['output_sha256']
            if 'cpu_border' in job:
                plan=job['cpu_border']
                if (plan['profile']!=scene.gain.CPU_BORDER_PROFILE or plan['input_sha256']!=job['output_sha256']
                        or plan['descriptor'][1]!=job['metadata']['address']
                        or plan['descriptor'][3]!=2*job['metadata']['stride_samples']
                        or plan['arguments'][:2]!=(0,0) or plan['arguments'][2]!=plan['arguments'][3]
                        or plan['accepted_tick'] is not None and (job['state']!='complete'
                            or not job['closed_tick']<=plan['accepted_tick']<=device.board.virtual_ticks)):
                    raise UnsupportedDevice('F1C CPU border handoff changed its sealed gain/descriptor chronology')
                if plan['accepted_tick'] is not None: expected=plan['output_sha256']
            if hashlib.sha256(raw).hexdigest()!=expected:
                raise UnsupportedDevice('F1C gain input/output SHA differs from its current owned RAW span')
        elif job['result_tick'] is None:
            raw=device.board.read_memory(job['metadata']['address'],job['metadata']['capacity'],cpu_id=0)
            if hashlib.sha256(raw).hexdigest()!=job['source_sha256']:
                raise UnsupportedDevice('F1C input changed before its declared statistics effect')
        elif job['state']=='complete' and job['request'][0] in (0,2):
            address=job['request'][10 if job['request'][0]==0 else 11]
            if bytes(device.board.read_memory(address,len(_expected_output(job)),cpu_id=0))!=_expected_output(job):
                raise UnsupportedDevice('F1C stock result copy changed after its complete reader')
    return job


def contains(device,address,size): return owner(device) is not None and BASE<=address<BASE+0x1000


def _reset_access(device,job,reset,operation,address,value):
    now=device.board.virtual_ticks
    if operation=='read' and address==BASE and reset['state'] in ('waiting','ready'):
        if reset['polls']>=idle.MAX_POLLS: raise UnsupportedDevice('F1C active idle polling exceeds its bounded law')
        reset['polls']+=1
        return 1 if reset['state']=='ready' else 0
    if operation=='write' and address==BASE+0xc8 and value==1 and reset['state']=='ready' and reset['polls']:
        reset.update(state='acknowledged',ack_tick=now); job['registers'][BASE+0xc8]=0
    elif operation=='write' and address==BASE+0xcc and value==0 and reset['state']=='acknowledged':
        reset.update(state='complete',disabled_tick=now); job['registers'][BASE+0xcc]=0
        job['state']='isr-closed' if reset is job['isr_reset'] else 'task-closed'
    else: raise UnsupportedDevice('F1C active reset/ACK/disable transaction reordered')


def access(device,operation,cpu,address,size,value=None):
    job=validate(device)
    if (job is None or cpu!=0 or type(cpu) is not int or size!=4 or address&3
            or operation not in ('read','write') or operation=='write' and (type(value) is not int or not 0<=value<1<<32)):
        raise UnsupportedDevice('F1C active processing owns only its CPU0 aligned WORD contract')
    if job['state'] in ('initializing','awaiting-configuration'):
        view=_initialization_view(device,job)
        result=idle.access(view,operation,cpu,address,size,value)
        device.phase_busy=view.phase_busy
        if job['initialization']['state']=='complete': job['state']='awaiting-configuration'
        _seal(job); return result
    if job['state']=='configuring':
        expected=job['plan'][job['step']]
        if (operation,address,value)!=expected: raise UnsupportedDevice('F1C upload differs from the exact whole mode configuration')
        _route(device,drained=True)
        if _words(device,0x1566708,13)!=job['request']: raise UnsupportedDevice('F1C request changed during its upload')
        result=job['registers'].get(address,0) if operation=='read' else None
        if operation=='write': job['registers'][address]=value
        job['step']+=1
        if job['step']==len(job['plan']): job['state']='configured'
        _seal(job); return result
    if job['state']=='configured' and address==BASE+4:
        core=device.board.cpus[0]
        if operation=='read':
            if (int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))!=(0x930018,0x10cdacc):
                raise UnsupportedDevice('F1C start read requires93000C from10CDAAC')
            return job['registers'][address]
        if value!=((job['registers'][address]&~7)|(job['request'][0]+2)):
            raise UnsupportedDevice('F1C start mode does not match its actual request')
        if int(core.reg_read(UC_ARM_REG_PC))!=0x930070 or job['scene'] is None:
            raise UnsupportedDevice('F1C start requires separately declared scene results at930070')
        validate(device,full=True); _route(device,drained=True)
        now=device.board.virtual_ticks
        if now+job['completion_ticks']>=1<<64:
            raise UnsupportedDevice('F1C completion deadline overflows')
        job['registers'][address]=value
        job.update(state='running',started_tick=now,due_tick=now+job['completion_ticks'])
        _seal(job); return
    if operation=='write' and address==BASE and value==0:
        core=device.board.cpus[0]; lr=int(core.reg_read(UC_ARM_REG_LR))
        if int(core.reg_read(UC_ARM_REG_PC))!=0x9319ac:
            raise UnsupportedDevice('F1C reset requires the actual93199C store')
        if job['state']=='irq-issued' and lr==0x10cda40:
            _route(device); reset_name='isr_reset'
            if device.board.interrupts.banks[0].active!=IRQ: raise UnsupportedDevice('F1C ISR reset precedes actual IRQ50 entry')
        elif job['state']=='isr-closed' and lr==0x10cdadc:
            _route(device,drained=True); reset_name='task_reset'
            if _words(device,0x173da24,1)!=(0,) or _words(device,0x1824ecc,1)!=(0,):
                raise UnsupportedDevice('F1C task close precedes the stock callback/flag consumer')
        else: raise UnsupportedDevice('F1C active reset has an unknown caller or unfinished prior phase')
        now=device.board.virtual_ticks
        if now+job['idle_ticks']>=1<<64:
            raise UnsupportedDevice('F1C reset idle deadline overflows')
        job[reset_name]=dict(state='waiting',requested_tick=now,due_tick=now+job['idle_ticks'],ready_tick=None,ack_tick=None,disabled_tick=None,polls=0)
        job['registers'][BASE]=0; job['state']='reset-'+reset_name; _seal(job); return
    if job['state'] in ('reset-isr_reset','reset-task_reset'):
        reset=job['isr_reset'] if job['state']=='reset-isr_reset' else job['task_reset']
        result=_reset_access(device,job,reset,operation,address,value); _seal(job); return result
    if job['state']=='task-closed' and operation=='read':
        _route(device,enabled=False,drained=True)
        if _words(device,0x173d9f0,1)!=(0,) or _words(device,0x173da24,1)!=(0,):
            raise UnsupportedDevice('F1C result read precedes stock unregister/callback clearing')
        addresses=_readout_addresses(job['request'][0])
        if job['result_step']>=len(addresses) or address!=addresses[job['result_step']]:
            raise UnsupportedDevice('F1C result read differs from its exact signed/count bank ordering')
        job['result_step']+=1; result=job['scene']['words'][address]
        if job['result_step']==len(addresses): job['state']='result-read'
        _seal(job); return result
    raise UnsupportedDevice('F1C active statistics has no qualified extra transaction or filter result')


def tick(device):
    job=owner(device)
    if job is None or job['state'] in ('configuring','configured','complete'): return False
    now=device.board.virtual_ticks
    if job['state'] in ('initializing','awaiting-configuration'):
        view=_initialization_view(device,job); changed=idle.tick(view)
        device.phase_busy=view.phase_busy
        return changed
    if job['state']=='running' and now>=job['due_tick']:
        validate(device,full=True); _route(device,drained=True)
        if 'output_sha256' in job:
            m=job['metadata']; raw=bytes(device.board.read_memory(m['address'],m['capacity'],cpu_id=0))
            effect=scene.prepare_gain(raw,source_metadata=m,parameters=job['parameters'],profile=scene.GAIN_PROFILE)
            if effect['identity']!=job['scene']['identity']:
                raise UnsupportedDevice('F1C gain effect differs from its sealed scene/input plan')
            for y in range(m['y0'],m['y1']+1):
                at=2*(y*m['stride_samples']+m['x0']); length=2*(m['x1']-m['x0']+1)
                device.board.write_memory(m['address']+at,effect['output'][at:at+length],cpu_id=0)
            current=device.board.read_memory(m['address'],m['capacity'],cpu_id=0)
            if hashlib.sha256(current).hexdigest()!=job['output_sha256']:
                raise UnsupportedDevice('F1C gain output DMA differs from its full owned-span SHA')
            job['output_tick']=now
        job['registers'].update(job['scene']['words'])
        job.update(state='irq-issued',result_tick=now); _seal(job)
        device.board.queue_irq(0,IRQ); return True
    if job['state'] in ('reset-isr_reset','reset-task_reset'):
        reset=job['isr_reset'] if job['state']=='reset-isr_reset' else job['task_reset']
        if reset['state']=='waiting' and now>=reset['due_tick']:
            validate(device); _route(device)
            reset.update(state='ready',ready_tick=now); job['registers'][BASE]=1; _seal(job); return True
    if job['state']=='task-closed' and job['request'][0] in (1,3):
        if (not device.board.interrupts.banks[0].enabled & (1<<IRQ)
                and _words(device,0x173d9f0,1)==(0,) and _words(device,0x173da24,1)==(0,)):
            validate(device); _route(device,enabled=False,drained=True)
            job.update(state='complete',closed_tick=now); device.phase_busy=False; _seal(job); return True
    if job['state']=='result-read':
        pc=int(device.board.cpus[0].reg_read(UC_ARM_REG_PC))
        if not 0x931008<=pc<0x93196c:
            validate(device); _route(device,enabled=False,drained=True)
            job.update(state='complete',closed_tick=now); device.phase_busy=False; _seal(job); return True
    return False


def next_event_tick(device,now):
    job=owner(device)
    if job is None: return None
    if job['state'] in ('initializing','awaiting-configuration'):
        return idle.next_event_tick(_initialization_view(device,job),now)
    if job['state']=='running': return max(now,job['due_tick'])
    if job['state'] in ('reset-isr_reset','reset-task_reset'):
        reset=job['isr_reset'] if job['state']=='reset-isr_reset' else job['task_reset']
        if reset['state']=='waiting': return max(now,reset['due_tick'])
    return None


def summary(device):
    job=validate(device,full=True)
    if job is None: return None
    if job['state'] in ('initializing','awaiting-configuration'):
        return dict(profile=PROFILE,state=job['state'],history_count=len(job['history']),next_mode=job['next_request'][0],
            initialization=idle.summary(_initialization_view(device,job)))
    result={k:deepcopy(job[k]) for k in ('profile','state','request','metadata','armed_tick','step','started_tick','due_tick','result_tick','isr_reset','task_reset','result_step','closed_tick','source_sha256','scene_sha256')}
    if 'output_sha256' in job:
        result.update({k:deepcopy(job[k]) for k in ('source_span','output_sha256','output_tick')})
    if 'cpu_border' in job: result['cpu_border']=deepcopy(job['cpu_border'])
    return result


def output_source(device):
    """A completed processing-owned RAW projection; never repair PFP history."""
    job=validate(device,full=True)
    if job is None or job['state']!='complete' or 'output_sha256' not in job: return None
    _route(device,enabled=False,drained=True)
    result=dict(source_span=deepcopy(job['source_span']),source_sha256=job['source_sha256'],
        output_sha256=job['output_sha256'],output_tick=job['output_tick'])
    if 'cpu_border' in job: result['cpu_border']=deepcopy(job['cpu_border'])
    return result


def handoff_to_processing(device):
    """Admit the predicted stock CPU border clear at the real F0 boundary.

    Old gain output remains sealed. This consumes a fully hashed post-clear
    span and never clears pixels, changes a descriptor or supplies a callback.
    """
    job=validate(device)
    core=device.board.cpus[0]
    boundary=(int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))
    if (job is None or job['state']!='complete' or job['request'][0]!=3
            or 'cpu_border' not in job or job['output_tick'] is None
            or boundary not in ((0x935080,0x10cdf28),(0x92be3c,0x10ce368))
            or device.state not in ('idle','armed')):
        raise UnsupportedDevice('F1C CPU handoff requires the qualified complete gain and real F0 admission')
    _route(device,enabled=False,drained=True)
    plan=job['cpu_border']; frame=plan['frame_address']
    if (_words(device,frame+0x88,7)!=plan['descriptor']
            or _words(device,frame+0x498,1)[0]!=plan['arguments'][2]
            or plan['profile']!=scene.gain.CPU_BORDER_PROFILE
            or plan['input_sha256']!=job['output_sha256']):
        raise UnsupportedDevice('F1C CPU handoff changed the recovered116BE88 source/border owner')
    raw=device.board.read_memory(job['metadata']['address'],job['metadata']['capacity'],cpu_id=0)
    if hashlib.sha256(raw).hexdigest()!=plan['output_sha256']:
        raise UnsupportedDevice('F1C CPU handoff differs from the exact predicted border-clear SHA')
    if plan['accepted_tick'] is None:
        plan['accepted_tick']=device.board.virtual_ticks; _seal(job)
    return output_source(device)
