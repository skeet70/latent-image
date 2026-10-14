"""Declared parameter-driven AF focus wire, encoder and physical IRQ inputs.

4401 and4403 remain stock drivers. Their accepted mode2 wire quantities,
not the high-level requested coordinate, determine counter4 edges. Counter0
gets one monitor edge per four encoder edges. Motion spans response_ticks;
that cadence and32:1 ratio are declared simulation laws, not measurements.
No guest pose, callbacks, AF classification or queue messages are supplied.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .pulse_devices import PulseOutputs

LIMIT = 128
DEFAULT_REGS = (0x2800,0x2c00,0x400,0x800,0x1400,0xc00,0x1800,0x1c00,0x2000,0x2400,0x1000,0,0x3800)
TAIL_REGS = (0x2800,0x2c00,0x400,0x800,0,0x1400,0xc00,0x1800,0x1c00,0x2000,0x2400,0x1000,0x3800)
STATES = ('collecting','pending-response','awaiting-feedback-cleanup','awaiting-stock-close','complete')
INITIAL_SCAN_BODY = (0xa09c7c, (-463) & 65535, 2, 1559, 2, 1, 8, 0)
FINAL_FIT_BODY = (0xa09b78, 272, 1, 1, 0, 12, 0)


def lens_note(e, kind, **data):
    """Passive observation hook for emulator.testbed.lens_trace; no effect without a trace or bus."""
    trace = getattr(e.board, 'lens_trace', None)
    if trace is not None:
        trace.note(e, kind, **data)
    else:
        bus = getattr(e.board, 'testbed_events', None)
        if bus is not None:
            bus.emit('lens.' + kind, **data)


def _continuous_body(body, q):
    return (body[0] == 0xa09c7c and q != 0 and body[2] in (1, 2)
            and 1 <= body[3] <= 65535 and body[4:6] == [2, 1] and body[7] == 0
            and (body[6] in (0, 5) or tuple(body) == INITIAL_SCAN_BODY))


def _step_body(body, q, zero):
    return (body[0] == 0xa09b78 and (zero and q == 0 and body[2:] == [2, 0, 0, 5, 0]
            or not zero and q != 0 and body[2:5] == [2, 2, 1]
            and body[5] in range(4) and body[6] == 0
            or _fitted_body(body)))


def _fitted_body(body):
    # The captured entry is -311 with +51 backlash and upper limit1450.
    # Requiring the full accepted relation below excludes limit clipping.
    return (len(body) == 7 and body[0] == 0xa09b78 and 1 <= body[1] <= 1710
            and body[2:] == [1, 1, 0, 12, 0])


def _head_control(m):
    return 0 if _fitted_body(m['body']) else 5


def _fitted_close(m):
    continuation = m.get('continuation')
    return (_fitted_body(m['body']) and type(continuation) is dict
            and continuation.get('state') == 0x14)


def _cleanup_sequence(m):
    feedback = m.get('feedback_control', 0xd3f0)
    timer = m.get('timer_control', 0xd3f0)
    if _fitted_close(m):
        return (feedback, 0xd800, 0x3c40, timer, timer)
    mode = 0x3cc0 if m.get('direction') else 0x3c80
    return (feedback, 0xd800, mode, 0xd800, mode, timer, timer)


def _sequence(m):
    return TAIL_REGS if m['driver'] == 0x4403 or m['selector'] == 3 else DEFAULT_REGS


def _wire_size(m):
    return 15 if _head_phase(m) else 19


def _head_phase(m):
    return m.get('continuation',{}).get('state')==0x13


def _driver_state(m):
    return m['continuation']['state'] if 'continuation' in m else 0x12 if m['driver']==0x4403 else 0x10


def _read(e,a,n=4,signed=False):
    return int.from_bytes(e.board.read_memory(a,n),'little',signed=signed)


def _digest(value):
    from .lens_af_motion import _digest as digest
    return digest(value)


def _pulses(e):
    found=[d for d in e.board.devices if isinstance(d,PulseOutputs)]
    if len(found)!=1 or found[0].counter_input_profile!=PulseOutputs.COUNTER_LENS_PROFILE:
        raise UnsupportedDevice('AF motion requires its single declared encoder/monitor device')
    return found[0]


def _route(e,line,vector):
    banks=e.board.interrupts.banks
    if (len(banks)!=2 or not banks[0].enabled&(1<<line) or banks[1].enabled&(1<<line)
            or banks[0].config.get(0x400+4*line)!=vector
            or banks[0].config.get(0x800+4*line)!=12
            or (banks[0].source_config>>line)&1!=1):
        raise UnsupportedDevice('AF motor physical vector/priority/trigger changed')


def _events(e,indices):
    if (type(indices) is not list or any(type(i) is not int or not 0<=i<len(e.words) for i in indices)
            or any(a>=b for a,b in zip(indices,indices[1:]))):
        raise UnsupportedDevice('AF motor wire index order changed')
    return [e.words[i] for i in indices]


def owned_indices(motor):
    if motor is None:return []
    return [i for m in motor['history']+[motor['current']] for i in m['word_indices']+m['cleanup_indices']]


def _validate_record(e,m,archived=False):
    if (type(m) is not dict or m.get('kind')!='mode2-focus'
            or m.get('state') not in STATES+('complete-zero','complete-without-timer','complete-phase')
            or type(m.get('driver')) is not int or m['driver'] not in (0x4401,0x4403)
            or type(m.get('body')) is not list or len(m['body'])!=(7 if m['driver']==0x4401 else 8)
            or any(type(v) is not int or not 0<=v<1<<32 for v in m['body'])
            or type(m.get('tick')) is not int or not 0<=m['tick']<=e.board.virtual_ticks
            or type(m.get('selector')) is not int or type(m.get('zero')) is not bool
            or (m.get('phase') is not None and (type(m['phase']) is not int or m['phase'] not in (0,1)))):
        raise UnsupportedDevice('AF motor retained driver/schema/clock changed')
    keys={'kind','state','driver','body','start','tick','selector','zero','phase',
          'word_indices','cleanup_indices','wire_sha256'}
    continuation=m.get('continuation')
    if continuation is not None:
        keys.add('continuation')
        if (type(continuation) is not dict or set(continuation)!={'state','head','correction'}
                or continuation['state'] not in (0x13,0x14) or type(continuation['state']) is not int
                or type(continuation['head']) is not int
                or not (1<=continuation['head']<=63 if _head_phase(m) else
                        continuation['head'] == (10 if _fitted_body(m['body']) else 0))
                or type(continuation['correction']) is not int or not 0<continuation['correction']<=32767
                or (m['driver'] != 0x4403 and not _fitted_body(m['body']))
                or m['zero'] or _head_phase(m) and m['phase'] is not None):
            raise UnsupportedDevice('AF motor body0 head phase schema changed')
    if len(m.get('word_indices',[]))==_wire_size(m):
        keys|={'quantity','edges','direction','requested_tick','due_tick','edges_delivered',
               'monitor_delivered','counter_generations','counter_baselines'}
    if m.get('state') not in ('collecting','pending-response','complete-zero'):
        keys.add('delivery_tick')
    if m.get('cleanup_indices'):keys.add('feedback_control')
    if len(m.get('cleanup_indices',[])) > (3 if _fitted_close(m) else 5):keys.add('timer_control')
    if set(m)!=keys:raise UnsupportedDevice('AF motor retained fields changed')
    body=m['body'];q=int.from_bytes((body[1]&65535).to_bytes(2,'little'),'little',signed=True)
    if m['driver']==0x4401:
        valid = _step_body(body, q, m['zero'])
        selector=body[5]
    else:
        valid=_continuous_body(body,q)
        selector=body[6]
    if not valid or m['selector']!=selector:raise UnsupportedDevice('AF motor retained admitted body changed')
    wire=_events(e,m['word_indices']);cleanup=_events(e,m['cleanup_indices'])
    if not m['zero'] and not _head_phase(m) and len(wire)>=3 and type(m['phase']) is not int:
        raise UnsupportedDevice('AF motor retained controller class changed')
    if (not wire or m.get('start')!=m['word_indices'][0] or len(wire)>_wire_size(m)
            or any(type(x.get('cpu')) is not int or x['cpu']!=0 or type(x.get('tick')) is not int
                   or not m['tick']<=x['tick']<=e.board.virtual_ticks for x in wire+cleanup)
            or _digest(wire)!=m.get('wire_sha256')):
        raise UnsupportedDevice('AF motor retained command/owner changed')
    if m['zero']:
        if (tuple(x['word'] for x in wire)!=(0xd3f0,)*len(wire) or len(wire)>2
                or m['driver']!=0x4401 or m['body'][2:]!=[2,0,0,5,0]
                or m['state']!=('complete-zero' if len(wire)==2 else 'collecting') or cleanup):
            raise UnsupportedDevice('AF motor zero4401 retained protocol changed')
        return
    seq = _sequence(m)
    for i,x in enumerate(wire):
        w=x['word']
        if _head_phase(m):
            ok=(w&0xfc00==seq[i] if i<13 else w==0xd800 if i==13 else w==0x3e80)
            if i==0:ok=ok and w&0x3ff==continuation['head']<<4
            elif i in (1,2,3):ok=ok and w&0x3ff==0
        elif i==0:ok=w==0xd800
        elif i==1:ok=w in (0x3c00,0x3c40,0x3c80,0x3cc0)
        elif i in (2,3):ok=w==0xd3f0+(4 if m['phase'] else 0)+2*(i-2)
        elif i<17:ok=w&0xfc00==seq[i-4]
        elif i==17:ok=w==0xd800
        else:ok=w in (0x3e80,0x3ec0)
        if continuation is not None and not _head_phase(m):
            if i==1:ok=ok and w==0x3c80
            elif i in (2,3):ok=ok and m['phase']==1
            elif i in (4,5):ok=ok and w&0x3ff==(continuation['head'] << 4 if i == 5 else 0)
            elif i==18:ok=ok and w==0x3ec0
        if not ok:raise UnsupportedDevice('AF motor retained whole command order changed')
    if (m['state']=='collecting')!=(len(wire)<_wire_size(m)):
        raise UnsupportedDevice('AF motor retained command completion changed')
    if len(wire)==_wire_size(m):
        regs={x['word']&0xfc00:x['word']&0x3ff for x in wire}
        q=regs[0x400]|regs[0x800]<<10
        valid_quantity=q==0 if _head_phase(m) else q==continuation['correction']*32 if continuation is not None else 0<q<=0x1fffe0 and q%32==0
        if tuple(body) == INITIAL_SCAN_BODY and continuation is None:
            valid_quantity = q == 463 * 32 and wire[-1]['word'] == 0x3ec0
        if _fitted_body(body):
            valid_quantity = (q == (body[1] + 51) * 32 and wire[-1]['word'] == 0x3e80
                if continuation is None else valid_quantity
                and continuation['correction'] == 51
                and (continuation['head'] == 10 if _head_phase(m) else True))
        if (type(m.get('quantity')) is not int or m['quantity']!=q or not valid_quantity
                or type(m.get('edges')) is not int or m['edges']!=q//32
                or type(m.get('direction')) is not int or m['direction']!=int(wire[-1]['word']==0x3ec0)
                or type(m.get('requested_tick')) is not int or m['requested_tick']!=wire[-1]['tick']
                or type(m.get('due_tick')) is not int or m['due_tick']!=m['requested_tick']+e.response_ticks
                or type(m.get('edges_delivered')) is not int or not 0<=m['edges_delivered']<=m['edges']
                or type(m.get('monitor_delivered')) is not int or m['monitor_delivered']!=m['edges_delivered']//4
                or type(m.get('counter_generations')) is not list or len(m['counter_generations'])!=2
                or any(type(g) is not int or g<0 for g in m['counter_generations'])
                or type(m.get('counter_baselines')) is not list or len(m['counter_baselines'])!=2
                or any(type(v) is not int or not 0<=v<65536 for v in m['counter_baselines'])):
            raise UnsupportedDevice('AF motor wire quantity/direction/feedback clock changed')
        if m['state'] not in ('collecting','pending-response'):
            if (m['edges_delivered']!=m['edges'] or type(m.get('delivery_tick')) is not int
                    or not m['due_tick']<=m['delivery_tick']<=e.board.virtual_ticks):
                raise UnsupportedDevice('AF motor final physical feedback changed')
    if cleanup and (type(m.get('feedback_control')) is not int or m['feedback_control'] not in (0xd3f0,0xd3f4)):
        raise UnsupportedDevice('AF motor feedback control word changed')
    expected = _cleanup_sequence(m)
    if len(cleanup) > (3 if _fitted_close(m) else 5) and (type(m.get('timer_control')) is not int or m['timer_control'] not in (0xd3f0,0xd3f4)):
        raise UnsupportedDevice('AF motor hold close control changed')
    if tuple(x['word'] for x in cleanup)!=expected[:len(cleanup)] or len(cleanup)>len(expected):
        raise UnsupportedDevice('AF motor retained feedback/timer cleanup changed')
    if (m['state']=='pending-response' and cleanup
            or m['state']=='awaiting-feedback-cleanup' and len(cleanup)>=3
            or m['state']=='awaiting-stock-close' and not 3<=len(cleanup)<len(expected)
            or m['state']=='complete-without-timer' and len(cleanup)!=(5 if _fitted_close(m) else 3)
            or m['state']=='complete' and len(cleanup)!=7
            or m['state']=='complete-phase' and len(cleanup)!=(0 if _head_phase(m) else 3)
            or archived and m['state'] not in ('complete','complete-zero','complete-without-timer','complete-phase')):
        raise UnsupportedDevice('AF motor cleanup lifecycle changed')


def validate(e,motor,*,pending=False):
    if (type(motor) is not dict or set(motor)!={'history','current'}
            or type(motor['history']) is not list or len(motor['history'])>=LIMIT):
        raise UnsupportedDevice('AF motor retained history bound/schema changed')
    for m in motor['history']:_validate_record(e,m,True)
    m=motor['current'];_validate_record(e,m)
    indices=owned_indices(motor)
    if len(indices)!=len(set(indices)):
        raise UnsupportedDevice('AF motor wire ownership overlaps')
    if pending and m['state']=='pending-response':
        _route(e,14,0xa0d2c0)
        if (_read(e,0xccdd9c,2)!=m['driver']
                or [_read(e,0xccdda8+i*4) for i in range(len(m['body']))]!=m['body']
                or _read(e,0xccde04)!=_driver_state(m)
                or _read(e,0xccdea8)!=2 or _read(e,0xccde1c)!=1
                or abs(_read(e,0xccde66,2,True))*32!=m['quantity']
                or _read(e,0xccdea4)!=m['direction']):
            raise UnsupportedDevice('AF motor saved native driver/accepted delta ownership changed')
        if 'continuation' in m and (_read(e,0xccde38)!=m['continuation']['head']
                or _read(e,0xccde20)!=(_head_control(m) if _head_phase(m) else 0)
                or _read(e,0xccdeb6,2,True)!=m['continuation']['correction']
                or _read(e,0xccde58)!=1 or _read(e,0xccde5c)!=(0 if _head_phase(m) else 1)):
            raise UnsupportedDevice('AF motor saved head/correction policy changed')
        if 'continuation' in m and not _head_phase(m) and (_read(e,0xccde66,2,True)!=-m['continuation']['correction'] or m['direction']!=1):
            raise UnsupportedDevice('AF motor reverse correction ownership changed')
        p=_pulses(e)
        for c in ((4,) if _head_phase(m) else (0,4)):p._counter_configuration(c)
        if _head_phase(m) and (p.enabled&1 or p.registers[p.CHANNEL_BASE+8]!=m['counter_baselines'][0]):
            raise UnsupportedDevice('AF motor head phase changed its stopped monitor prefix')
        expected=[(m['counter_baselines'][0]+m['monitor_delivered'])&65535,
                  (m['counter_baselines'][1]+m['edges_delivered'])&65535]
        if [p.counter_expected[c] for c in (0,4)]!=expected:
            raise UnsupportedDevice('AF motor delivered counter feedback changed')
        if [p.counter_generations[c] for c in (0,4)]!=m['counter_generations']:
            raise UnsupportedDevice('AF motor counter ownership changed')
    return motor


def _begin(e):
    cls=_read(e,0xccdd70,2);n=7 if cls==0x4401 else 8 if cls==0x4403 else 0
    if not n:raise UnsupportedDevice('AF motor requires actual latched4401/4403 request')
    body=[_read(e,0xccdd7c+i*4) for i in range(n)]
    q=_read(e,0xccdd80,2,True)
    zero=cls==0x4401 and q==0 and body[2:]==[2,0,0,5,0]
    if cls==0x4401:
        ok = _step_body(body, q, zero)
        selector=body[5]
    else:
        ok=_continuous_body(body,q)
        selector=body[6]
    if (not ok or not zero and q==0 or _read(e,0xccde1c)!=1
            or _read(e,0xccde04) not in ((0x12,) if cls==0x4403 else (0,0x10,0x18))):
        raise UnsupportedDevice('AF motor latched body/homed driver class remains unresolved')
    if tuple(body) == INITIAL_SCAN_BODY and (
            _read(e,0xccde64,2,True) != 152 or _read(e,0xccde66,2,True) != -463
            or _read(e,0xccdea8) != 2 or _read(e,0xccdea4) != 1):
        raise UnsupportedDevice('AF initial negative scan pose/accepted delta/encoder owner changed')
    if _fitted_body(body) and (
            _read(e, 0xccde64, 2, True) != -311 or _read(e, 0xccde66, 2, True) != q + 51
            or _read(e, 0xccdea8) != 2 or _read(e, 0xccdea4) != 0
            or _read(e, 0xccde58) != 1 or _read(e, 0xccde5c) != 1
            or _read(e, 0xccdeb6, 2, True) != 51 or _read(e, 0xccde20) != 0
            or _read(e, 0xccde38) != 0):
        raise UnsupportedDevice('AF final fitted step requires its observed pose/backlash/encoder owner')
    return dict(kind='mode2-focus',state='collecting',driver=cls,body=body,
        start=len(e.words),tick=e.board.virtual_ticks,selector=selector,zero=zero,
        phase=None,word_indices=[],cleanup_indices=[])


def _begin_head_phase(e,old):
    if (_read(e,0xccdd70,2)!=0x4408 or _read(e,0xccdd7c)!=0
            or _read(e,0xccdd9c,2)!=old['driver']
            or (old['driver'] != 0x4403 and not _fitted_body(old['body']))
            or [_read(e,0xccdda8+i*4) for i in range(len(old['body']))]!=old['body']
            or _read(e,0xccde04)!=0x13 or _read(e,0xccde58)!=1 or _read(e,0xccde5c)!=0
            or _read(e,0xccde66,2,True)!=0 or _read(e,0xccdea8)!=2 or _read(e,0xccdea4)!=0
            or _read(e,0xccde20)!=_head_control(old) or not 1<=_read(e,0xccde38)<=63
            or not 0<_read(e,0xccdeb6,2,True)<=32767 or _pulses(e).settle_timer is not None):
        raise UnsupportedDevice('AF motor head phase requires actual calibrated4408/body0 state13')
    if _fitted_body(old['body']) and (
            _read(e, 0xccde64, 2, True) != -311 + old['body'][1] + 51 or _read(e, 0xccde38) != 10
            or _read(e, 0xccdeb6, 2, True) != 51):
        raise UnsupportedDevice('AF final fitted step head/correction calibration changed')
    return dict(kind='mode2-focus',state='collecting',driver=old['driver'],body=list(old['body']),
        start=len(e.words),tick=e.board.virtual_ticks,selector=old['selector'],zero=False,
        phase=None,word_indices=[],cleanup_indices=[],
        continuation=dict(state=0x13,head=_read(e,0xccde38),correction=_read(e,0xccdeb6,2,True)))


def _begin_backlash_phase(e,old):
    correction=old['continuation']['correction']
    if (_read(e,0xccdd70,2)!=0x4408 or _read(e,0xccdd7c)!=0
            or _read(e,0xccdd9c,2)!=old['driver']
            or (old['driver'] != 0x4403 and not _fitted_body(old['body']))
            or [_read(e,0xccdda8+i*4) for i in range(len(old['body']))]!=old['body']
            or _read(e,0xccde04)!=0x14 or _read(e,0xccde58)!=1 or _read(e,0xccde5c)!=0
            or _read(e,0xccde66,2,True)!=-correction or _read(e,0xccdea8)!=2
            or _read(e,0xccdea4)!=1 or _read(e,0xccde38)!=old['continuation']['head']
            or _read(e,0xccdeb6,2,True)!=correction or _pulses(e).settle_timer is not None
            or old['cleanup_indices']):
        raise UnsupportedDevice('AF motor correction requires delivered head and actual4408/body0 state14')
    return dict(kind='mode2-focus',state='collecting',driver=old['driver'],body=list(old['body']),
        start=len(e.words),tick=e.board.virtual_ticks,selector=old['selector'],zero=False,
        phase=None,word_indices=[],cleanup_indices=[],
        continuation=dict(state=0x14,head=10 if _fitted_body(old['body']) else 0,correction=correction))


def receive(e,motor,core,word):
    if core!=0:raise UnsupportedDevice('AF motor supports its actual CPU0 driver only')
    if motor is None:
        result=dict(history=[],current=_begin(e))
    else:
        validate(e,motor);result=deepcopy(motor);old=result['current']
        if old['state']=='awaiting-feedback-cleanup' and _head_phase(old):
            result['current']=_begin_backlash_phase(e,old)
            old['state']='complete-phase';result['history'].append(old)
            if len(result['history'])>=LIMIT:raise UnsupportedDevice('AF motor history bound reached')
        elif old['state']=='awaiting-stock-close' and not _fitted_close(old) and _read(e,0xccdd70,2)==0x4408 and _read(e,0xccdd7c)==0:
            result['current']=_begin_head_phase(e,old)
            old['state']='complete-phase';result['history'].append(old)
            if len(result['history'])>=LIMIT:raise UnsupportedDevice('AF motor history bound reached')
        elif old['state'] in ('complete','complete-zero','complete-without-timer','awaiting-stock-close') and _read(e,0xccdd70,2) in (0x4401,0x4403):
            if old['state']=='awaiting-stock-close':
                if _pulses(e).settle_timer is not None:
                    raise UnsupportedDevice('AF motor previous holdtimer still owns the wire')
                old['state']='complete-without-timer'
            result['history'].append(old)
            if len(result['history'])>=LIMIT:raise UnsupportedDevice('AF motor history bound reached')
            result['current']=_begin(e)
    m=result['current']
    if m['state']=='collecting':
        i=len(m['word_indices']);seq = _sequence(m)
        if _head_phase(m):
            ok=(word&0xfc00==seq[i] if i<13 else word==0xd800 if i==13 else i==14 and word==0x3e80)
            if i==0:ok=ok and word&0x3ff==m['continuation']['head']<<4
            elif i in (1,2,3):ok=ok and word&0x3ff==0
        elif m['zero']:ok=i<2 and word==0xd3f0
        elif i==0:ok=word==0xd800
        elif i==1:ok=word in (0x3c00,0x3c40,0x3c80,0x3cc0)
        elif i==2:
            ok=word in (0xd3f0,0xd3f4)
            if ok:m['phase']=int(word==0xd3f4)
        elif i==3:ok=word==0xd3f0+(4 if m['phase'] else 0)+2
        elif i<17:ok=word&0xfc00==seq[i-4]
        elif i==17:ok=word==0xd800
        else:ok=i==18 and word in (0x3e80,0x3ec0)
        if 'continuation' in m and not _head_phase(m):
            if i==1:ok=ok and word==0x3c80
            elif i in (2,3):ok=ok and m['phase']==1
            elif i in (4,5):ok=ok and word&0x3ff==(m['continuation']['head'] << 4 if i == 5 else 0)
            elif i==18:ok=ok and word==0x3ec0
        if not ok:raise UnsupportedDevice('AF motor whole command order unresolved')
        m['word_indices'].append(len(e.words))
        events=[e.words[i] for i in m['word_indices'][:-1]]+[dict(cpu=core,word=word,tick=e.board.virtual_ticks)]
        m['wire_sha256']=_digest(events)
        if m['zero'] and len(events)==2:m['state']='complete-zero'
        elif not m['zero'] and len(events)==_wire_size(m):
            regs={x['word']&0xfc00:x['word']&0x3ff for x in events};q=regs[0x400]|regs[0x800]<<10
            valid_quantity=q==0 if _head_phase(m) else q==m['continuation']['correction']*32 if 'continuation' in m else 0<q<=0x1fffe0 and q%32==0
            if (_read(e,0xccdea8)!=2 or _read(e,0xccde04)!=_driver_state(m)
                    or not valid_quantity or q!=abs(_read(e,0xccde66,2,True))*32
                    or _read(e,0xccdea4)!=int(word==0x3ec0)):
                raise UnsupportedDevice('AF motor accepted mode2 wire quantity/direction unresolved')
            p=_pulses(e)
            m.update(state='pending-response',quantity=q,edges=q//32,direction=int(word==0x3ec0),
                requested_tick=e.board.virtual_ticks,due_tick=e.board.virtual_ticks+e.response_ticks,
                edges_delivered=0,monitor_delivered=0,counter_generations=[p.counter_generations[c] for c in (0,4)],
                counter_baselines=[p.counter_expected[c] for c in (0,4)])
            lens_note(e,'command',driver=m['driver'],body=m['body'],quantity=q,edges=q//32,direction=m['direction'],
                      span=e.response_ticks,tick=e.board.virtual_ticks,profile='strict')
            lens_note(e,'move_start',edges=q//32,direction=m['direction'],tick=e.board.virtual_ticks,
                      due_tick=m['due_tick'])
        return result
    if m['state'] in ('awaiting-feedback-cleanup','awaiting-stock-close'):
        i=len(m['cleanup_indices'])
        if m['state']=='awaiting-stock-close' and (_read(e,0xccdd70,2)!=0x4408
                or _read(e,0xccdd7c)!=(0 if _fitted_close(m) else 3)):
            raise UnsupportedDevice('AF motor closure requires its real4408 caller body')
        if i==0:
            if word not in (0xd3f0,0xd3f4):raise UnsupportedDevice('AF motor feedback stop control changed')
            m['feedback_control']=word
        if i==(3 if _fitted_close(m) else 5):
            if word not in (0xd3f0,0xd3f4):raise UnsupportedDevice('AF motor timer stop control changed')
            m['timer_control']=word
        seq = _cleanup_sequence(m)
        if i>=len(seq) or word!=seq[i]:raise UnsupportedDevice('AF motor feedback/timer cleanup order changed')
        m['cleanup_indices'].append(len(e.words))
        if i==2:m['state']='awaiting-stock-close'
        elif i==len(seq)-1:m['state']='complete-without-timer' if _fitted_close(m) else 'complete'
        return result
    raise UnsupportedDevice('AF motor premature or unknown later wire class')


def next_event_tick(e,motor):
    if motor is None:return None
    if type(motor) is not dict or type(motor.get('current')) is not dict:
        raise UnsupportedDevice('AF motor timer identity changed')
    m=motor['current']
    if m.get('state') not in STATES+('complete-zero','complete-without-timer'):
        raise UnsupportedDevice('AF motor timer lifecycle changed')
    if m['state']!='pending-response':return None
    zero_head='continuation' in m and m.get('edges')==0
    if (type(m.get('edges')) is not int or m['edges']<0 or m['edges']==0 and not zero_head
            or type(m.get('edges_delivered')) is not int or (m['edges_delivered']!=0 if zero_head else not 0<=m['edges_delivered']<m['edges'])
            or type(m.get('requested_tick')) is not int or type(m.get('due_tick')) is not int
            or m['due_tick']!=m['requested_tick']+e.response_ticks):
        raise UnsupportedDevice('AF motor pending clock/edge ownership changed')
    if zero_head:return m['due_tick']
    return m['requested_tick']+(e.response_ticks*(m['edges_delivered']+1)+m['edges']-1)//m['edges']


def tick(e,motor):
    due=next_event_tick(e,motor)
    if due is None or e.board.virtual_ticks<due:return
    validate(e,motor,pending=True);m=motor['current'];p=_pulses(e)
    target=min(m['edges'],(e.board.virtual_ticks-m['requested_tick'])*m['edges']//e.response_ticks)
    extra=target-m['edges_delivered'];monitor=target//4-m['monitor_delivered']
    if extra:p.add_counter_edges(4,extra,source=PulseOutputs.COUNTER_LENS_PROFILE)
    if monitor:p.add_counter_edges(0,monitor,source=PulseOutputs.COUNTER_LENS_PROFILE)
    m.update(edges_delivered=target,monitor_delivered=target//4)
    if target==m['edges']:
        e.board.queue_irq(0,14);m.update(state='awaiting-feedback-cleanup',delivery_tick=e.board.virtual_ticks)
        lens_note(e,'move_end',edges=m['edges'],monitor=m['monitor_delivered'],direction=m['direction'],
                  tick=e.board.virtual_ticks)
        e.board.record('Declared AF motor final physical response',irq=14,quantity=m['quantity'],
            limits='Wire-derived external counts; stock ISR/consumer and separate holdtimer own closure')


# ---------------------------------------------------------------------------
# Generic stock-move profile (declared, unmeasured). The strict ledger above
# admits only whole-driver-qualified bodies. This ledger instead parses the
# transmitted focus words with a small grammar of stock productions, checks each
# against the firmware's own RAM (accepted delta, direction, driver copy, state)
# and drives the same wire-derived encoder/monitor edges and IRQ14. It never
# writes guest RAM, callbacks or positions, and refuses everything else.
# ---------------------------------------------------------------------------
GENERIC_PROFILE = 'declared-generic-stock-focus-moves-unmeasured'
GENERIC_WORD_LIMIT = 1 << 14
GENERIC_RECORD_LIMIT = 4096
RAMP_PERIODS = 0xccde24  # Five stock ramp periods; 15625000/period is the rate byte times 10.
RAMP_STAGE_PULSES = 15
DEFAULT_TICKS_PER_SECOND = 5_000_000  # 400000 ticks ~ 273 pulses at 3500/s, the strict default.
DEFAULT_PULSES_PER_SECOND = 2000
# ``start_ticks`` (default 0, the original behavior): latency between the stock main-move command and the first pulse of
# its ramp, ticks of the board clock. Declared; the camera trials fit about 26.7 ms (research/af-timing/README.md).
DEFAULT_IRIS_TICKS_PER_UNIT = 7143  # 56 coordinate units (index 0..11) ~ the strict 400000 ticks.
CTL = (0xd3f0, 0xd3f4)
MODES = (0x3c00, 0x3c40, 0x3c80, 0x3cc0)
FINAL = (0x3e80, 0x3ec0)
PATTERN_LENGTH = dict(cmd=19, head=15, zero=2, fb=3, tailL=4, tailS=2, astop=3, afb=2)
IDLE = ('cmd', 'zero', 'tailL')    # A zero request may close with a bare hold tail instead of the two control words.
# A queued step may start directly after aborted-scan feedback; _g_move still verifies the actual new stock request.
SUCCESSORS = {None: IDLE, 'cmd': ('fb', 'astop'), 'astop': ('afb',), 'afb': ('cmd', 'tailL', 'tailS'), 'head': ('fb', 'cmd', 'tailL'), 'zero': IDLE, 'tailL': IDLE, 'tailS': IDLE,
              'fb': ('head', 'cmd', 'tailL', 'tailS', 'zero')}
NATIVE_WORDS = (0xccde04, 0xccde1c, 0xccde20, 0xccde38, 0xccde58, 0xccde5c, 0xccdea4, 0xccdea8, 0xccdeb6)


def generic_parameters(ticks_per_second=DEFAULT_TICKS_PER_SECOND,
                       default_pulses_per_second=DEFAULT_PULSES_PER_SECOND, settle_ticks=None, iris=True,
                       iris_ticks_per_unit=DEFAULT_IRIS_TICKS_PER_UNIT, iris_settle_ticks=None, start_ticks=0):
    ints = (ticks_per_second, default_pulses_per_second, iris_ticks_per_unit, start_ticks)
    if (any(type(v) is not int for v in ints) or not 1000 <= ticks_per_second <= 1 << 32 or not 0 <= start_ticks <= 1 << 32
            or not 1 <= default_pulses_per_second <= 65535 or not 1 <= iris_ticks_per_unit <= 1 << 24
            or type(iris) is not bool
            or any(v is not None and (type(v) is not int or not 1 <= v <= 1 << 32)
                   for v in (settle_ticks, iris_settle_ticks))):
        raise ValueError('Generic lens timing parameters must be bounded integers')
    return dict(ticks_per_second=ticks_per_second, default_pulses_per_second=default_pulses_per_second,
                settle_ticks=settle_ticks, iris=iris, iris_ticks_per_unit=iris_ticks_per_unit,
                iris_settle_ticks=iris_settle_ticks, start_ticks=start_ticks)


def generic_selected(e):
    r = e.requests['focus']
    return type(r) is dict and r.get('profile') == GENERIC_PROFILE


def _stock_step(body):
    """Any stock A08F84/420B/4401 request: speed index 0..2 and bounded stock options."""
    return (len(body) == 7 and body[0] == 0xa09b78 and body[2] in (0, 1, 2) and body[3] in (0, 1, 2)
            and body[4] in (0, 1) and body[5] < 32 and body[6] == 0)


# Completion callbacks of the stock 4403 continuous producers: AF scan A08084 -> A09C7C, and the manual-focus
# drive (lens message 4210, A03974 -> A07E8C, used by the MF ring/RTMF path) -> A0ACA4 (posts 4237).
CONTINUOUS_CALLBACKS = (0xa09c7c, 0xa0aca4)


def _stock_continuous(body, q):
    return (len(body) == 8 and body[0] in CONTINUOUS_CALLBACKS and body[2] in (0, 1, 2) and 1 <= body[3] <= 65535
            and body[4] in (0, 1, 2) and body[5] in (0, 1) and body[6] < 32 and body[7] == 0)


def _g_driver(e, latched):
    """Stock driver class and body from the latched request or the running driver copy."""
    cls = _read(e, 0xccdd70 if latched else 0xccdd9c, 2)
    if cls not in (0x4401, 0x4403):
        raise UnsupportedDevice('Generic focus move requires an actual latched4401/4403 driver')
    base = 0xccdd7c if latched else 0xccdda8
    body = [_read(e, base+4*i) for i in range(7 if cls == 0x4401 else 8)]
    q = int.from_bytes((body[1] & 65535).to_bytes(2, 'little'), 'little', signed=True)
    if (not (_stock_step(body) if cls == 0x4401 else _stock_continuous(body, q))
            or q == 0 and cls == 0x4401 and body[3:5] != [0, 0]):    # Stock zero-delta callers pass a4=a5=0.
        raise UnsupportedDevice('Generic focus move parameters are not stock A08F84/A08084/A07E8C values')
    return cls, body, q


def _pulse_ticks(e, params, cls, body):
    """Declared ticks per logical pulse for the five 15-pulse ramp stages."""
    if cls == 0x4403:
        rates = [body[3]] * 5  # The explicit 4403 rate is pulses/s.
    else:
        periods = [_read(e, RAMP_PERIODS+4*i) for i in range(5)]
        rates = ([2 * 15625000 // x for x in periods] if all(0 < x <= 15625000 for x in periods)
                 else [params['default_pulses_per_second']] * 5)
    return [max(1, -(-params['ticks_per_second'] // max(1, r))) for r in rates]


def _cum(pt, n):
    total = 0
    for k, d in enumerate(pt):
        c = min(n, RAMP_STAGE_PULSES) if k < 4 else n
        total += c * d
        n -= c
        if n <= 0:
            break
    return total


def _count(pt, elapsed, edges):
    n = 0
    for k, d in enumerate(pt):
        cap = RAMP_STAGE_PULSES if k < 4 else edges
        c = min(cap, elapsed // d, edges - n)
        n += c
        elapsed -= c * d
        if c < cap or n >= edges:
            break
    return n


def _g_ok(kind, ws):
    """True when ws is a prefix (or the whole) of the stock production `kind`."""
    if kind[0] == 'i':
        return _g_iris_ok(kind, ws)
    n = len(ws)
    if n > PATTERN_LENGTH[kind]:
        return False
    if kind in ('zero', 'tailS'):
        return ws[0] in CTL and all(w == ws[0] for w in ws)
    if kind == 'afb':      # after a stock stop: the mode half of the feedback stop (its control word was in astop)
        return ws[0] == 0xd800 and (n < 2 or ws[1] in MODES)
    if kind == 'astop':    # stock stop of a running move: D801, a control word, one register word (not a mode)
        return ws[0] == 0xd801 and (n < 2 or ws[1] in CTL) and (n < 3 or ws[2] & 0xfc00 == 0)
    if kind == 'fb':
        return ws[0] in CTL and (n < 2 or ws[1] == 0xd800) and (n < 3 or ws[2] in MODES)
    if kind == 'tailL':
        return (ws[0] == 0xd800 and (n < 2 or ws[1] in MODES) and (n < 3 or ws[2] in CTL)
                and (n < 4 or ws[3] == ws[2]))
    base = 4 if kind == 'cmd' else 0
    if kind == 'cmd' and not (ws[0] == 0xd800 and (n < 2 or ws[1] in MODES) and (n < 3 or ws[2] in CTL)
                              and (n < 4 or ws[3] == ws[2]+2)):
        return False
    seq = TAIL_REGS if n > base+4 and ws[base+4] & 0xfc00 == 0 else DEFAULT_REGS
    if any(ws[i] & 0xfc00 != seq[i-base] for i in range(base, min(n, base+13))):
        return False
    last = base + 13
    return (not kind == 'head' or n < 2 or not ws[1] & 0x3ff) and (n <= last or ws[last] == 0xd800) \
        and (n <= last+1 or ws[last+1] in FINAL)


def _g_candidates(prev, ws, axis='records'):
    first = IRIS_IDLE if axis == 'iris' else IDLE
    return [k for k in (SUCCESSORS[prev['kind']] if prev else first) if _g_ok(k, ws)
            and (k != 'astop' or prev.get('aborted'))]


def _g_quantity(ws, base):
    regs = {w & 0xfc00: w & 0x3ff for w in ws[base:base+13]}
    return regs[0x400] | regs[0x800] << 10


def _g_native(e):
    return {hex(a): _read(e, a) for a in NATIVE_WORDS} | dict(delta=_read(e, 0xccde66, 2, True))


def _g_latch(e, bodies=(0,)):
    if _read(e, 0xccdd70, 2) != 0x4408 or _read(e, 0xccdd7c) not in bodies:
        raise UnsupportedDevice('Generic focus word requires its real4408 stock caller body')
    return _read(e, 0xccdd7c)


def _g_words(e, rec):
    return [e.words[i]['word'] for i in range(rec['start'], rec['start']+rec['count'])]


def _g_finish(e, params, recs, rec, ws, word):
    """A complete production: check it against firmware RAM; schedule motion for commands."""
    kind = rec['kind']
    if kind == 'zero':
        cls, body, q = _g_driver(e, True)
        if q != 0 or _read(e, 0xccde1c) != 1:
            raise UnsupportedDevice('Generic zero focus move requires the actual zero stock request')
        rec.update(state='done', driver=cls, body=body)
        lens_note(e, 'zero', body=body, tick=e.board.virtual_ticks)
    elif kind == 'fb':
        rec.update(state='done', mode=ws[2])
    elif kind == 'astop':
        rec.update(state='done', remaining=ws[2])
    elif kind == 'afb':
        rec.update(state='done', mode=ws[1])
    elif kind in ('tailL', 'tailS'):
        if recs and recs[-1]['kind'] in ('fb', 'head', 'afb'):
            rec.update(state='done', caller_body=_g_latch(e, (0, 3)))
        else:      # A zero request answered by the bare hold close.
            _, _, q = _g_driver(e, True)
            if q != 0 or kind != 'tailL' or _read(e, 0xccde1c) != 1:
                raise UnsupportedDevice('Generic focus hold tail requires a stock close or an actual zero request')
            rec.update(state='done', caller_body=None)
            lens_note(e, 'zero', body=_g_driver(e, True)[1], tick=e.board.virtual_ticks)
    else:
        _g_move(e, params, recs, rec, ws, word)


def _g_move(e, params, recs, rec, ws, word):
    prev, head = recs[-1] if recs else None, rec['kind'] == 'head'
    state, delta = _read(e, 0xccde04), _read(e, 0xccde66, 2, True)
    if head:
        mains = [r for r in recs if r['kind'] == 'cmd' and r['phase'] == 'main']
        if (not mains or prev['kind'] != 'fb' or state not in (0x13, 0x17) or delta != 0
                or ws[0] & 0x3ff != _read(e, 0xccde38) << 4 or not 1 <= _read(e, 0xccde38) <= 63):
            raise UnsupportedDevice('Generic focus head command requires its actual state13/17 stock caller')
        _g_latch(e)
        phase, cls, body = 'head', mains[-1]['driver'], mains[-1]['body']
    elif prev is not None and prev['kind'] == 'head' and state == 0x14:
        _g_latch(e)
        phase, cls, body = 'correction', prev['driver'], prev['body']
    elif state in (0x10, 0x12):
        phase = 'main'
        cls, body, _ = _g_driver(e, True)
        if state != (0x12 if cls == 0x4403 else 0x10) or _read(e, 0xccde1c) != 1:
            raise UnsupportedDevice('Generic focus move requires its actual homed stock driver state')
    else:
        raise UnsupportedDevice('Generic focus command does not match any stock driver state')
    running = _g_driver(e, False)
    if running[:2] != (cls, body) or _read(e, 0xccdea8) != 2:
        raise UnsupportedDevice('Generic focus running driver copy or encoder mode changed')
    q, direction = _g_quantity(ws, 0 if head else 4), int(word == 0x3ec0)
    if (q != abs(delta) * 32 or q % 32 or q > 0x1fffe0 or _read(e, 0xccdea4) != direction
            or not q and phase != 'head'
            or phase == 'correction' and (q // 32 != _read(e, 0xccdeb6, 2, True))):
        raise UnsupportedDevice('Generic focus wire quantity/direction differs from the accepted stock delta')
    p, now = _pulses(e), e.board.virtual_ticks
    for c in ((0, 4) if q else ()):
        p._counter_configuration(c)
    pt = _pulse_ticks(e, params, cls, body)
    start = params['start_ticks'] if phase == 'main' and q else 0       # latency before the first pulse of a main move
    span = start + _cum(pt, q // 32) if q else params['settle_ticks'] or e.response_ticks
    # Physical pose at the request (host-side bookkeeping, never written to the guest): the end of the previous
    # move this model delivered, else the firmware's committed pose.
    committed = _read(e, 0xccde64, 2, True)
    rec.update(start_position=_physical(recs, committed, now), committed_start=committed,
               sign=-1 if direction else 1)
    rec.update(state='pending', phase=phase, driver=cls, body=body, quantity=q, edges=q // 32,
               direction=direction, requested_tick=now, due_tick=now+span, span=span, pulse_ticks=pt, start_ticks=start,
               edges_delivered=0, monitor_delivered=0, native=_g_native(e),
               counter_generations=[p.counter_generations[c] for c in (0, 4)],
               counter_baselines=[p.counter_expected[c] for c in (0, 4)])
    lens_note(e, 'command', phase=phase, driver=cls, body=body, quantity=q, edges=q // 32,
            direction=direction, pulse_ticks=pt, span=span, tick=now)
    lens_note(e, 'move_start', phase=phase, edges=q // 32, direction=direction, tick=now, due_tick=now+span)


def _physical(recs, committed, tick):
    """Declared physical focus position at ``tick`` from the generic ledger.

    The lens model owns the physical motion: a pending command moves the lens from its recorded start by
    ``sign * pulses`` where ``pulses`` is the declared ramp law (``_count``) at the elapsed ticks, the same
    law that drives the encoder edges the stock counter reads. After the command it stays at the end of the
    last delivered move. With no generic move yet the lens is where the firmware committed it.
    """
    for rec in reversed(recs):
        if 'start_position' not in rec:
            continue
        if rec['state'] == 'pending' and rec['edges']:
            elapsed = max(0, tick - rec['requested_tick'] - rec.get('start_ticks', 0))
            return rec['start_position'] + rec['sign'] * _count(rec['pulse_ticks'], elapsed, rec['edges'])
        if rec['state'] == 'pending':
            return rec['start_position']
        return rec['start_position'] + rec['sign'] * (rec['edges'] + rec.get('coast', 0))
    return committed


def generic_physical_position(e, tick=None):
    """Physical focus position (pulses, committed-pose frame) the generic lens model has the lens at, or None."""
    if not generic_selected(e):
        return None
    ledger = e.requests['focus']
    return _physical(ledger['records'], _read(e, 0xccde64, 2, True),
                     e.board.virtual_ticks if tick is None else tick)


def generic_motion_state(e, tick=None):
    """``dict(position, committed, moving, phase, start, end, direction)`` for diagnostics, or None."""
    if not generic_selected(e):
        return None
    tick = e.board.virtual_ticks if tick is None else tick
    recs = e.requests['focus']['records']
    committed = _read(e, 0xccde64, 2, True)
    pending = recs[-1] if recs and recs[-1].get('state') == 'pending' and recs[-1].get('edges') else None
    return dict(position=_physical(recs, committed, tick), committed=committed, moving=pending is not None,
                phase=None if pending is None else pending['phase'],
                end=None if pending is None else pending['start_position'] + pending['sign'] * pending['edges'])


def generic_validate(e, ledger):
    recs, iris = ledger['records'], ledger['iris']
    if (type(recs) is not list or type(iris) is not list or len(recs) > GENERIC_RECORD_LIMIT
            or len(iris) > GENERIC_RECORD_LIMIT or any(type(r) is not dict for r in recs[-2:] + iris[-2:])
            or sum(r['count'] for r in recs) + sum(r['count'] for r in iris) != len(e.words) - ledger['anchor']
            or any(r['kind'] not in (None, *PATTERN_LENGTH) for r in recs[-2:] + iris[-2:])):
        raise UnsupportedDevice('Generic lens retained records or wire ownership changed')
    return ledger


def generic_receive(e, ledger, core, word):
    """A replacement ledger with this word added to its axis' production lattice."""
    if core != 0:
        raise UnsupportedDevice('Generic lens moves support the actual CPU0 driver only')
    axis = 'iris' if 0x4000 <= word < 0xc000 else 'records'
    if axis == 'iris' and not ledger['params']['iris']:
        raise UnsupportedDevice('Generic focus-move profile has no iris wire contract')
    result = dict(ledger, records=list(ledger['records']), iris=list(ledger['iris']))
    recs, other = result[axis], result['iris' if axis == 'records' else 'records']
    collecting = [r for r in (recs[-1:] + other[-1:]) if r['state'] == 'collecting']
    if collecting and collecting[0] is not (recs[-1] if recs else None):
        raise UnsupportedDevice('Generic lens words of both axes interleave inside one stock production')
    last = recs[-1] if recs else None
    if last is not None and last['state'] == 'collecting':
        rec, prev = dict(last), (recs[-2] if len(recs) > 1 else None)
        recs[-1] = rec
    else:
        if last is not None and last['state'] == 'pending' and axis == 'records' and word == 0xd801:
            last = recs[-1] = _g_abort(e, last)      # the stock stop word cuts the running move short
        elif last is not None and last['state'] == 'pending':
            raise UnsupportedDevice('Generic lens word arrived during unfinished stock motion on its axis')
        if len(recs) >= GENERIC_RECORD_LIMIT:
            raise UnsupportedDevice('Generic lens record bound reached')
        rec, prev = dict(kind=None, state='collecting', start=len(e.words), count=0), last
        recs.append(rec)
    ws = _g_words(e, rec) + [word]
    kinds = _g_candidates(prev, ws, axis)
    if not kinds:
        raise UnsupportedDevice('Generic lens word is outside the stock command/cleanup grammar')
    if sorted(kinds) == ['tailS', 'zero'] and len(ws) == 2:  # The latched stock message decides.
        kinds = ['tailS' if _read(e, 0xccdd70, 2) == 0x4408 else 'zero']
    if sorted(kinds) == ['iclose', 'icmd'] and len(ws) == 1:      # 7C00: a zero accepted delta is the lone close
        kinds = ['iclose' if _read(e, 0xca7c22, 2) == 0 else 'icmd']
    rec['count'] += 1
    rec['kind'] = kinds[0] if len(kinds) == 1 else None
    if len(kinds) == 1 and len(ws) == PATTERN_LENGTH[kinds[0]]:
        (_g_finish_iris if axis == 'iris' else _g_finish)(e, ledger['params'], recs[:-1], rec, ws, word)
    return result


# Declared: after a stock stop word the lens runs on by this many pulses before it rests. Evidence: the stock stop
# handler commits the counter pose and then one pulse further along the travel (committed 49 then 48 after a
# negative move that stopped at 49, ``research/emulator-rom/afs-focus-motion.md``).
STOP_COAST_PULSES = 1


def _g_abort(e, m):
    """The running move ``m`` is stopped by the stock D801 word: the lens halts where the declared ramp law has it.

    Pending ramp edges are delivered up to now, the remainder is cancelled, and the move is recorded as
    delivered at its truncated length (``aborted`` carries the planned edge count). The stock cleanup that
    follows is the ``astop`` production; no final-response IRQ14 is asserted (it belongs to completed moves)."""
    if m['kind'] != 'cmd' or not m.get('edges') or m['phase'] not in ('main', 'correction'):
        raise UnsupportedDevice('Generic focus stop word D801 requires a running stock main/correction move')
    p, now = _pulses(e), e.board.virtual_ticks
    done = _count(m['pulse_ticks'], max(0, now - m['requested_tick'] - m.get('start_ticks', 0)), m['edges'])
    now_native = _g_native(e)
    if now_native != dict(m['native'], **{hex(0xccde04): 0x15}):     # driver state 0x15: the stock stop handler
        raise UnsupportedDevice('Generic focus stock driver ownership changed before its stop word')
    extra, monitor = done - m['edges_delivered'], done // 4 - m['monitor_delivered']
    if extra:
        p.add_counter_edges(4, extra, source=PulseOutputs.COUNTER_LENS_PROFILE)
    if monitor:
        p.add_counter_edges(0, monitor, source=PulseOutputs.COUNTER_LENS_PROFILE)
    _route(e, 14, 0xa0d2c0)
    e.board.queue_irq(0, 14)
    e.board.record('Declared generic focus motor stop response', irq=14, edges=done, planned=m['edges'],
                   limits='Wire-derived external counts up to the stock D801 stop; stock ISR/consumer own closure')
    m = dict(m, edges_delivered=done, monitor_delivered=done // 4, aborted=m['edges'], edges=done, state='delivered',
             delivery_tick=now, quantity=done * 32, coast=STOP_COAST_PULSES)
    lens_note(e, 'abort', phase=m['phase'], edges=done, planned=m['aborted'], tick=now)
    lens_note(e, 'move_end', phase=m['phase'], edges=done, monitor=done // 4, direction=m['direction'], tick=now,
              aborted=True)
    return m


def _g_pending(ledger):
    return [(axis, ledger[axis][-1]) for axis in ('records', 'iris')
            if ledger[axis] and ledger[axis][-1]['state'] == 'pending']


def generic_next_tick(e, ledger):
    dates = [m['requested_tick'] + m.get('start_ticks', 0) + _cum(m['pulse_ticks'], m['edges_delivered']+1) if m.get('edges') else m['due_tick']
             for _, m in _g_pending(ledger)]
    return min(dates) if dates else None


def generic_tick(e, ledger):
    for axis, m in _g_pending(ledger):
        if axis == 'iris':
            _g_tick_iris(e, m)
        else:
            _g_tick_focus(e, m)


def _g_tick_focus(e, m):
    due = m['requested_tick'] + m.get('start_ticks', 0) + _cum(m['pulse_ticks'], m['edges_delivered']+1) if m['edges'] else m['due_tick']
    if e.board.virtual_ticks < due:
        return
    _route(e, 14, 0xa0d2c0)
    if _g_native(e) != m['native']:
        raise UnsupportedDevice('Generic focus stock driver ownership changed during motion')
    p = _pulses(e)
    if m['edges']:
        for c in (0, 4):
            p._counter_configuration(c)
        if ([p.counter_generations[c] for c in (0, 4)] != m['counter_generations']
                or [p.counter_expected[c] for c in (0, 4)] != [
                    (m['counter_baselines'][0]+m['monitor_delivered']) & 65535,
                    (m['counter_baselines'][1]+m['edges_delivered']) & 65535]):
            raise UnsupportedDevice('Generic focus delivered counter feedback changed')
    elapsed = e.board.virtual_ticks - m['requested_tick']
    target = _count(m['pulse_ticks'], max(0, elapsed - m.get('start_ticks', 0)), m['edges']) if m['edges'] else 0
    extra, monitor = target - m['edges_delivered'], target // 4 - m['monitor_delivered']
    if extra:
        p.add_counter_edges(4, extra, source=PulseOutputs.COUNTER_LENS_PROFILE)
    if monitor:
        p.add_counter_edges(0, monitor, source=PulseOutputs.COUNTER_LENS_PROFILE)
    m.update(edges_delivered=target, monitor_delivered=target // 4)
    if target == m['edges'] and elapsed >= m['span']:
        e.board.queue_irq(0, 14)
        m.update(state='delivered', delivery_tick=e.board.virtual_ticks)
        e.board.record('Declared generic focus motor final physical response', irq=14, quantity=m['quantity'],
                       limits='Wire-derived external counts; stock ISR/consumer and separate holdtimer own closure')
        lens_note(e, 'move_end', phase=m['phase'], edges=m['edges'], monitor=m['monitor_delivered'],
                  direction=m['direction'], tick=e.board.virtual_ticks)


# Iris: 15-word command [7Cxx option, 13 configuration registers, 7E80/7EC0 strobe], one cleanup word after the
# physical IRQ12, and a lone 7C00 close for a zero-travel request. Position/index/callbacks stay the firmware's.
IRIS_REGS = (0x4000, 0x5400, 0x4c00, 0x5800, 0x5c00, 0x6000, 0x6400, 0x5000, 0x7800, 0x6800, 0x6c00, 0x4400, 0x4800)
IRIS_OPTIONS = IRIS_CLEANUP = (0x7c00, 0x7c40, 0x7c80, 0x7cc0)
IRIS_FINAL = (0x7e80, 0x7ec0)
PATTERN_LENGTH.update(icmd=15, iclean=1, iclose=1)
IRIS_IDLE = ('icmd', 'iclose')
SUCCESSORS.update({'icmd': ('iclean',), 'iclean': IRIS_IDLE, 'iclose': IRIS_IDLE})
IRIS_NATIVE = ((0xca7b8c, 2), (0xca7c20, 2), (0xca7c22, 2), (0xca7cb0, 1), (0xc97664, 4))


def _g_iris_ok(kind, ws):
    n = len(ws)
    if n > PATTERN_LENGTH[kind]:
        return False
    if kind == 'iclean':
        return ws[0] in IRIS_CLEANUP
    if kind == 'iclose':
        return ws[0] == 0x7c00
    return (ws[0] in IRIS_OPTIONS and all(ws[i] & 0xfc00 == IRIS_REGS[i-1] for i in range(1, min(n, 14)))
            and (n < 15 or ws[14] in IRIS_FINAL))


def _g_iris_native(e):
    return {hex(a): _read(e, a, n) for a, n in IRIS_NATIVE} | dict(
        delta=_read(e, 0xca7c22, 2, True), body=[_read(e, 0xca7b98+4*i) for i in range(6)])


def _g_finish_iris(e, params, recs, rec, ws, word):
    kind, prev = rec['kind'], recs[-1] if recs else None
    if kind == 'iclean':
        rec.update(state='done')
        lens_note(e, 'iris', phase='cleanup', event='cleanup', word=word, tick=e.board.virtual_ticks)
        return
    if kind == 'iclose':      # A zero-travel request: no wire motion and no response.
        if _read(e, 0xca7b8c, 2) != 0x4501 or _read(e, 0xca7c22, 2) != 0 or _read(e, 0xca7cb0, 1) > 31:
            raise UnsupportedDevice('Generic iris close requires its actual zero-travel stock 4501 request')
        rec.update(state='done')
        lens_note(e, 'iris', phase='close', event='close', tick=e.board.virtual_ticks)
        return
    delta, body = _read(e, 0xca7c22, 2, True), [_read(e, 0xca7b98+4*i) for i in range(6)]
    regs = {w & 0xfc00: w & 0x3ff for w in ws[1:14]}
    q = regs[0x4400] | regs[0x4800] << 10
    if (_read(e, 0xca7b8c, 2) != 0x4501 or body[0] != 0xa09e44 or body[1] > 31 or _read(e, 0xca7cb0, 1) > 31
            or _read(e, 0xc97664) != 1 or q != abs(delta) * 32 or q > 0x1fffe0
            or (ws[14] == 0x7e80) != (delta < 0)):
        raise UnsupportedDevice('Generic iris command differs from its stock 4501 request/accepted coordinate delta')
    from .lens_measurement_iris import _route
    _route(e, 0)
    now = e.board.virtual_ticks
    span = max(params['iris_settle_ticks'] or e.response_ticks, abs(delta) * params['iris_ticks_per_unit'])
    rec.update(state='pending', quantity=q, units=abs(delta), direction=int(delta < 0), body=body, option=ws[0],
               requested_tick=now, due_tick=now+span, span=span, native=_g_iris_native(e))
    lens_note(e, 'iris', phase='move', event='start', body=body, quantity=q, direction=rec['direction'],
              tick=now, due_tick=now+span)


def _g_tick_iris(e, m):
    if e.board.virtual_ticks < m['due_tick']:
        return
    from .lens_measurement_iris import _route
    _route(e, 0)
    if _g_iris_native(e) != m['native']:
        raise UnsupportedDevice('Generic iris stock driver ownership changed during motion')
    e.board.queue_irq(0, 12)
    m.update(state='delivered', delivery_tick=e.board.virtual_ticks)
    e.board.record('Declared generic iris physical response', irq=12, quantity=m['quantity'],
                   limits='Stock ISR/consumer owns index, position and callbacks')
    lens_note(e, 'iris', phase='move', event='end', quantity=m['quantity'], direction=m['direction'],
              tick=e.board.virtual_ticks)


def generic_abort_selected(ledger):
    """Stock-aborted move cleanup: the three-word ``astop`` is complete and its C000 timer may be armed."""
    recs = ledger.get('records') or []
    return bool(recs) and recs[-1]['kind'] == 'astop' and recs[-1]['state'] == 'done'


def generic_hold_selected(ledger):
    """The pulse device may arm its C001 hold timer after a delivered motor IRQ until the stock close ends."""
    recs = ledger.get('records') or []
    if not recs or recs[-1]['state'] == 'pending':
        return False
    for i in range(len(recs)-1, max(len(recs)-5, -1), -1):
        if recs[i]['state'] == 'delivered':
            return all(r['kind'] in ('fb', 'astop', 'afb', None, 'tailL', 'tailS')
                       and (r['state'] != 'done' or r['kind'] in ('fb', 'astop', 'afb'))
                       for r in recs[i+1:])
    return recs[-1]['state'] == 'collecting' and recs[-1]['kind'] in (None, 'tailL')
