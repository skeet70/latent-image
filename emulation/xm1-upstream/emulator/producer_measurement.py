"""One explicitly admitted generation4 measurement producer lifetime.

The retired generation3 arm, both AF statistics and histogram histories are
immutable. Only audited desired fields and the complete10CAEA0(mask3F)
descriptor/arm/enable transaction are retained here. Admission supplies no
guest bytes, status, callbacks or interrupt. Numeric output is a separate law.
"""
from copy import copy, deepcopy

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import afs_measurement_data


PROFILE = 'declared-afs-generation4-measurement-producer-unmeasured'
RETIREMENT_PROFILE = 'stock-afs-second-measurement-idle-retirement-unmeasured'
CONFIG_LIMIT = 8192
RESET_POLL_LIMIT = 65536
CONTINUATION_WINDOW_ORDER=tuple(0xfff28000+offset for offset in (0x78,0x84,*range(0x88,0xf8,4)))


def _reset_validate(device,record):
    reset=record['reset']
    if reset is None:return None
    now=device.board.virtual_ticks
    if (type(reset) is not dict or set(reset) != {'after_access_count','requested_tick','due_tick',
            'acknowledged_tick','state','accesses','current_word'}
            or reset['after_access_count'] != 1 or len(record['accesses']) < 1
            or (record['accesses'][0]['operation'],record['accesses'][0]['address'],record['accesses'][0]['word'])
               != ('read',device.GLOBAL,0x10030000)
            or type(reset['requested_tick']) is not int or not record['armed_tick'] <= reset['requested_tick'] <= now
            or type(device.stop_ticks) is not int or not 1 <= device.stop_ticks < 1 << 64
            or reset['due_tick'] != reset['requested_tick'] + device.stop_ticks or reset['due_tick'] >= 1 << 64
            or reset['acknowledged_tick'] is not None and (type(reset['acknowledged_tick']) is not int
                or not reset['due_tick'] <= reset['acknowledged_tick'] <= now)
            or type(reset['accesses']) is not list or not 1 <= len(reset['accesses']) <= 1+RESET_POLL_LIMIT):
        raise UnsupportedDevice('Measurement reset requires exact stock GLOBAL read/clear and bounded bit28 delay')
    previous=reset['requested_tick'];word=0;completed=False
    for step,event in enumerate(reset['accesses']):
        expected=0 if step==0 or reset['acknowledged_tick'] is None or event['tick'] < reset['acknowledged_tick'] else 0x10000000
        if (type(event) is not dict or set(event) != {'operation','address','word','cpu','tick','step'}
                or (event['operation'],event['address'],event['cpu'],event['step'],event['word'])
                   != ('write' if step==0 else 'read',device.GLOBAL,0,step,expected)
                or type(event['tick']) is not int or not previous <= event['tick'] <= now
                or step==0 and event['tick'] != reset['requested_tick'] or completed):
            raise UnsupportedDevice('Measurement reset differs from complete clear/poll transaction')
        previous=event['tick'];word=expected;completed=expected==0x10000000
    if reset['acknowledged_tick'] is not None:word=0x10000000
    state='completed' if completed else 'waiting-ack' if reset['acknowledged_tick'] is None else 'acknowledged-awaiting-poll'
    if reset['state'] != state or reset['current_word'] != word:
        raise UnsupportedDevice('Measurement reset projection/completion changed')
    return word


def _reset_access(device,record,operation,core,address,size,value):
    reset=record['reset']
    if reset is None:
        if (operation,address,value) != ('write',device.GLOBAL,0):return False
        marker=_marker(device,full=False)
        if (record['request'] is not None or len(record['accesses']) != 1
                or marker['state'] not in ('asserted','acknowledged')
                or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured'
                or device.registers[device.GLOBAL] != 0x10030000):
            raise UnsupportedDevice('Measurement GLOBAL reset requires the actual source and first stock read')
        now=device.board.virtual_ticks
        reset=dict(after_access_count=1,requested_tick=now,due_tick=now+device.stop_ticks,
                   acknowledged_tick=None,state='waiting-ack',accesses=[],current_word=0)
        if reset['due_tick'] >= 1 << 64:raise UnsupportedDevice('Measurement reset delay escapes the virtual clock')
    elif reset['state'] == 'completed':return False
    if (address != device.GLOBAL or operation != ('write' if not reset['accesses'] else 'read')
            or core != 0 or type(core) is not int or size != 4 or type(size) is not int
            or len(reset['accesses']) >= 1+RESET_POLL_LIMIT):
        raise UnsupportedDevice('Measurement reset must finish its next CPU0 clear/poll access')
    word=device.registers[address] if operation=='read' else value
    if word != (0x10000000 if reset['acknowledged_tick'] is not None else 0):
        raise UnsupportedDevice('Measurement reset poll differs from its actual delayed response')
    event=dict(operation=operation,address=address,word=word,cpu=core,tick=device.board.virtual_ticks,step=len(reset['accesses']))
    reset['accesses'].append(event)
    if operation=='write':
        old=device.registers[address];device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    elif word==0x10000000:reset['state']='completed'
    record['reset']=reset
    device.board.record('Measurement producer GLOBAL reset transaction retained',**event,state=reset['state'],
        limits='Existing explicit stop bit28 law only; no DMA/data/IRQ/callback')
    return True


def tick(device,board):
    raw=device.rearm.get('measurement') if type(device.rearm) is dict else None
    if raw is not None and raw.get('data') is not None and raw['data'].get('state')!='acknowledged':
        if board is not device.board:raise UnsupportedDevice('Measurement data tick belongs to another board')
        return afs_measurement_data.tick(device,raw,board)
    if raw is None or raw.get('reset') is None or raw['reset'].get('state')=='completed':return False
    record=validate(device)
    if record is None or record['reset'] is None or record['reset']['state']=='completed':return False
    if board is not device.board:raise UnsupportedDevice('Measurement reset tick belongs to another board')
    return _reset_tick(device,record,board)


def _reset_tick(device,record,board):
    reset=record['reset']
    if reset['state']=='waiting-ack' and board.virtual_ticks >= reset['due_tick']:
        device.registers[device.GLOBAL]=0x10000000
        reset.update(acknowledged_tick=board.virtual_ticks,current_word=0x10000000,state='acknowledged-awaiting-poll')
        board.record('Measurement GLOBAL reset bit28 acknowledged',profile=record['profile'],
            limits='Unmeasured existing stop latency, bit28 only; no data/IRQ/callback')
    return True


def due(device):
    raw=device.rearm.get('measurement') if type(device.rearm) is dict else None
    if raw is not None and raw.get('data') is not None:
        return afs_measurement_data.due(raw)
    if raw is None or raw.get('reset') is None or raw['reset'].get('state')!='waiting-ack':return None
    record=validate(device,check_history=False)
    if record is None or record['reset'] is None:return None
    reset=record['reset']
    return reset['due_tick'] if reset['state']=='waiting-ack' else None


def _source_records(board,*,full=True):
    """Local MMIO view, or the complete consuming source-history audit.

    Passive setters own current words and a small physical input identity.
    Replaying retired producer/image/data histories belongs to admission,
    actual timed effects and public validation. Nothing is cached or retained
    here: each MMIO call checks this same-board bounded live identity anew.
    """
    from . import afs_frame_source as source
    if full:return source.requests(board)
    devices=[item for item in board.devices if type(item).__name__=='AfsFrameSource']
    if len(devices)!=1 or devices[0].board is not board:
        raise UnsupportedDevice('Measurement MMIO requires one same-board physical source')
    current=devices[0].request
    if type(current) is not dict or type(current.get('history',[])) is not list:
        raise UnsupportedDevice('Measurement MMIO source history shape changed')
    records=current.get('history',[])+[current]
    from . import afs_startup_scope
    inputs = devices[0]._device('InputProcessingControl')
    identities = tuple(row[:5] for row in afs_startup_scope.identities(inputs.event_profile))
    if not 3<=len(records)<=5:
        raise UnsupportedDevice('Measurement MMIO exceeds the five qualified source identities')
    previous=-1
    for record,identity in zip(records,identities):
        if (type(record) is not dict
                or tuple(record.get(key) for key in ('profile','generation','ordinal','attempt','frame'))!=identity
                or any(type(record.get(key)) is not int for key in
                       ('generation','ordinal','attempt','frame','cpu','source','ack_selector',
                        'requested_tick','due_tick','delay_ticks'))
                or (record['cpu'],record['source'],record.get('physical_irq'),record['ack_selector'])!=(0,0,'0x27',1)
                or record.get('state') not in ('waiting','asserted','acknowledged')
                or record is not current and record['state']!='acknowledged'
                or not previous<=record['requested_tick']<=board.virtual_ticks
                or record['delay_ticks']<=0 or record['due_tick']!=record['requested_tick']+record['delay_ticks']
                or record['due_tick']>=1<<64):
            raise UnsupportedDevice('Measurement MMIO physical source identity/timing changed')
        if record['state']=='waiting':
            if record is not current or record.get('asserted_tick') is not None or record.get('acknowledged_tick') is not None:
                raise UnsupportedDevice('Measurement MMIO retains an unobserved historical source')
        elif (type(record.get('asserted_tick')) is not int
                or not record['due_tick']<=record['asserted_tick']<=board.virtual_ticks
                or record['state']=='acknowledged' and (type(record.get('acknowledged_tick')) is not int
                   or not record['asserted_tick']<=record['acknowledged_tick']<=board.virtual_ticks)):
            raise UnsupportedDevice('Measurement MMIO source assertion/ACK timing changed')
        previous=record['requested_tick']
    if any(record['state']!='acknowledged' for record in records[:2]):
        raise UnsupportedDevice('Measurement MMIO lost its closed generation3 prefix')
    return records


def _marker(device,*,full=True):
    from .afs_frame_source import MEASUREMENT_PROFILE
    records = _source_records(device.board,full=full)
    matches=[record for record in records if record['profile']==MEASUREMENT_PROFILE
             and (record['generation'],record['frame'])==(4,7)]
    if (not 3 <= len(records) <= 5 or len(matches)!=1
            or any(record['state'] != 'acknowledged' for record in records[:2])):
        raise UnsupportedDevice('Measurement producer requires its exact third source identity')
    return matches[0]


def source_anchor(device,expected=None):
    """Pure activation identity: never recurse into the source or old history."""
    record=device.rearm.get('measurement') if type(device.rearm) is dict else None
    request=record.get('request') if type(record) is dict else None
    if (record is None or record.get('profile')!=PROFILE or record.get('cpu')!=0
            or type(record.get('current_words')) is not dict
            or type(request) is not dict
            or set(request)!={'step','state','accesses','started_tick','armed_tick','enabled_tick','route'}
            or request['step']!=22 or request['state']!=device._preview_request_state(22)
            or type(request['accesses']) is not list or len(request['accesses'])!=22
            or type(request['enabled_tick']) is not int
            or not record['armed_tick']<=request['enabled_tick']<=device.board.virtual_ticks):
        raise UnsupportedDevice('Measurement source anchor requires the complete real22-access activation')
    previous=record['armed_tick']
    for step,event in enumerate(request['accesses']):
        kind,address,value=device._preview_request_sequence()[step]
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or (event['operation'],event['address'],event['cpu'],event['step'])!=(kind,address,0,step)
                or type(event['word']) is not int or type(event['old_word']) is not int
                or (not device._config_valid(address,event['word']) if value is None else event['word']!=value)
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement activation anchor lost its actual ordered transaction')
        previous=event['tick']
    if (request['started_tick']!=request['accesses'][0]['tick']
            or request['armed_tick']!=request['accesses'][17]['tick']
            or request['enabled_tick']!=request['accesses'][21]['tick']):
        raise UnsupportedDevice('Measurement source anchor activation timing changed')
    identity=dict(profile=record['profile'],cpu=record['cpu'],initial_input_sha256=record['input_sha256'],
                  previous_sha256=record['previous_sha256'],desired_words=record['current_words'],activation=request)
    anchor=dict(enabled_tick=request['enabled_tick'],sha256=hp.digest(identity))
    if expected is not None and (type(expected) is not dict or set(expected)!={'enabled_tick','sha256'} or expected!=anchor):
        raise UnsupportedDevice('Measurement historical selector differs from its sole retained activation')
    return anchor


def _pixel_epoch(device,frame=8,*,full=True):
    from .afs_frame_source import epoch_for
    if full:return epoch_for(device.board,frame)
    return _source_epoch(device.board,frame)


def _source_epoch(board,frame):
    matches=[record for record in _source_records(board,full=False) if record['frame']==frame]
    if not matches or matches[0]['state'] not in ('asserted','acknowledged'):return None
    return {key:matches[0][key] for key in ('profile','generation','ordinal','attempt','frame',
        'cpu','source','physical_irq','asserted_tick')}


def _rotation_validate(device,record,words,*,key='rotation',frame=8,base=0x5fecd410,full_source=True):
    rotation=record.get(key)
    if rotation is None:return
    epoch=_pixel_epoch(device,frame,full=full_source);sequence=device.REARM_ROTATION_ORDER
    offsets=(0x850,0,0x1990,0x1d90,0x2190,0x21b0,0x23b0,0x1850,0x800)
    if (type(rotation) is not dict or set(rotation)!={'step','state','source_epoch','accesses'}
            or epoch is None or rotation['source_epoch']!=epoch
            or type(rotation['step']) is not int or not 1<=rotation['step']<=len(sequence)
            or rotation['state']!=('completed' if rotation['step']==len(sequence) else 'in-progress')
            or type(rotation['accesses']) is not list or len(rotation['accesses'])!=rotation['step']
            or record.get('data') is None or epoch['asserted_tick']<=record['request']['enabled_tick']):
        raise UnsupportedDevice('Measurement rotation requires exactly one actual post-enable pixel epoch')
    previous=epoch['asserted_tick']
    for step,event in enumerate(rotation['accesses']):
        address=sequence[step]
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or (event['operation'],event['address'],event['word'],event['cpu'],event['step'])
                    !=('write',address,base+offsets[step],0,step)
                or event['old_word']!=words[address] or type(event['tick']) is not int
                or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement rotation differs from the stock nine-store B tuple')
        words[address]=event['word'];previous=event['tick']


def _rotation_access(device,record,operation,core,address,value):
    second=record.get('data',{}).get('profile')==afs_measurement_data.CONTINUATION_PROFILE
    key='continuation_rotation' if second else 'rotation'
    frame=9 if second else 8;base=0x5fecaa60 if second else 0x5fecd410
    rotation=record.get(key);sequence=device.REARM_ROTATION_ORDER
    if record['request'] is None or record['request']['step']!=22:return False
    if rotation is not None and rotation['state']=='completed':return False
    if rotation is None and (operation!='write' or address!=sequence[0]):return False
    epoch=_pixel_epoch(device,frame,full=False)
    if (second and (len(record.get('data_history',[]))!=1 or record.get('rotation',{}).get('state')!='completed')):
        raise UnsupportedDevice('Measurement second rotation requires the closed first data and B tuple')
    if (record['request'] is None or record['request']['step']!=22 or record.get('data') is None
            or epoch is None or epoch['asserted_tick']<=record['request']['enabled_tick']):
        raise UnsupportedDevice('Measurement destination rotation requires its armed post-enable scene input')
    step=0 if rotation is None else rotation['step']
    offsets=(0x850,0,0x1990,0x1d90,0x2190,0x21b0,0x23b0,0x1850,0x800)
    if operation!='write' or address!=sequence[step] or type(value) is not int or value!=base+offsets[step]:
        raise UnsupportedDevice('Measurement destination rotation requires its next exact B-bank store')
    if rotation is None:rotation=dict(step=0,state='in-progress',source_epoch=epoch,accesses=[])
    old=device.registers[address]
    event=dict(operation=operation,address=address,word=value,old_word=old,cpu=core,tick=device.board.virtual_ticks,step=step)
    rotation['accesses'].append(event);rotation['step']+=1
    rotation['state']='completed' if rotation['step']==len(sequence) else 'in-progress'
    record[key]=rotation;device.registers[address]=value
    device.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
    device.board.record('Measurement next-bank destination retained',**event,
                        limits='One actual pixel-source rotation; the completed numeric bank remains latched')
    return True


def _late_validate(device,record,words):
    events=record.get('late_configuration')
    if events is None:return
    rotation=record.get('rotation')
    if (type(events) is not list or not 1<=len(events)<=CONFIG_LIMIT
            or rotation is None or rotation['state']!='completed'):
        raise UnsupportedDevice('Measurement next-slot configuration requires the complete source-owned rotation')
    previous=rotation['accesses'][-1]['tick']
    for step,event in enumerate(events):
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or event['address'] not in device.KNOWN_DESIRED_CONFIG
                or (event['operation'],event['cpu'],event['step'])!=('write',0,step)
                or event['old_word']!=words[event['address']] or type(event['word']) is not int
                or not device._known_pair_valid(event['address'],event['old_word'],event['word'])
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement next-slot configuration changed its desired-field transaction')
        words[event['address']]=event['word'];previous=event['tick']


def _continuation_validate(device,record,words):
    events=record.get('continuation_configuration')
    if events is None:return
    rotation=record.get('continuation_rotation')
    if (type(events) is not list or not 1<=len(events)<=CONFIG_LIMIT
            or rotation is None or rotation['state']!='completed'
            or record.get('data',{}).get('profile')!=afs_measurement_data.CONTINUATION_PROFILE):
        raise UnsupportedDevice('Measurement next-slot fields require the complete Source9-owned A rotation')
    previous=rotation['accesses'][-1]['tick']
    forbidden={device.GLOBAL,device.BASE+4,device.BASE+8}
    for step,event in enumerate(events):
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or event['address'] not in device.KNOWN_DESIRED_CONFIG or event['address'] in forbidden
                or step<len(CONTINUATION_WINDOW_ORDER) and event['address']!=CONTINUATION_WINDOW_ORDER[step]
                or (event['operation'],event['cpu'],event['step'])!=('write',0,step)
                or event['old_word']!=words[event['address']] or type(event['word']) is not int
                or not device._known_pair_valid(event['address'],event['old_word'],event['word'])
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement next-slot setter differs from whole9349F8/934CE4 or known passive fields')
        words[event['address']]=event['word'];previous=event['tick']


def _continuation_access(device,record,operation,core,address,value):
    rotation=record.get('continuation_rotation');events=record.get('continuation_configuration',[])
    step=len(events);old=device.registers[address]
    if (operation!='write' or rotation is None or rotation['state']!='completed' or step>=CONFIG_LIMIT
            or address in (device.GLOBAL,device.BASE+4,device.BASE+8)
            or step<len(CONTINUATION_WINDOW_ORDER) and address!=CONTINUATION_WINDOW_ORDER[step]
            or type(value) is not int or not device._known_pair_valid(address,old,value)):
        raise UnsupportedDevice('Measurement next-slot configuration requires complete Source9 rotation and stock bounded passive setter')
    event=dict(operation=operation,address=address,word=value,old_word=old,cpu=core,tick=device.board.virtual_ticks,step=step)
    events.append(event);record['continuation_configuration']=events;device.registers[address]=value
    device.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
    device.board.record('Source9 next-slot desired field retained',**event,
        limits='Whole30-store window40 prefix then existing bounded masked passive fields; BankB data remains latched; no reset/enable/job/status')
    return True


def _continuation_mode_validate(device,record,words):
    mode=record.get('continuation_mode9')
    if mode is None:return
    events=record.get('continuation_configuration',[])
    if (type(mode) is not dict or set(mode)!={'after_config_count','step','state','accesses'}
            or type(mode['after_config_count']) is not int
            or not len(CONTINUATION_WINDOW_ORDER)<=mode['after_config_count']<=len(events)
            or type(mode['step']) is not int or not 1<=mode['step']<=3
            or mode['state']!=('completed' if mode['step']==3 else 'in-progress')
            or type(mode['accesses']) is not list or len(mode['accesses'])!=mode['step']
            or record.get('continuation_rotation',{}).get('state')!='completed'):
        raise UnsupportedDevice('Measurement mode9 requires its Source9 window and whole getter/setter')
    previous=events[mode['after_config_count']-1]['tick']
    for step,event in enumerate(mode['accesses']):
        kind='write' if step==2 else 'read'
        old=words[device.GLOBAL]
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or (event['operation'],event['address'],event['cpu'],event['step'])!=(kind,device.GLOBAL,0,step)
                or type(event['old_word']) is not int or event['old_word']!=old
                or type(event['word']) is not int or not device._global_valid(event['word'])
                or (event['word']!=old if kind=='read' else (event['word']^old)&~0x70000!=0)
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement mode9 differs from whole9336A4/9335D0 bits16..18')
        words[device.GLOBAL]=event['word'];previous=event['tick']
    if len(events)>mode['after_config_count'] and events[mode['after_config_count']]['tick']<previous:
        raise UnsupportedDevice('Measurement later desired fields precede complete mode9 getter/setter')


def _continuation_mode_access(device,record,operation,core,address,value):
    if address!=device.GLOBAL:return False
    mode=record.get('continuation_mode9');events=record.get('continuation_configuration',[])
    if mode is not None and mode['state']=='completed':
        if operation=='read':return True
        raise UnsupportedDevice('Measurement later GLOBAL control requires separate producer retirement')
    if (record.get('continuation_rotation',{}).get('state')!='completed'
            or len(events)<len(CONTINUATION_WINDOW_ORDER)):
        raise UnsupportedDevice('Measurement mode9 requires its completed Source9 window/rotation')
    step=0 if mode is None else mode['step'];kind='write' if step==2 else 'read'
    old=device.registers[address];word=old if operation=='read' else value
    if (operation!=kind or type(word) is not int or not device._global_valid(word)
            or kind=='write' and (word^old)&~0x70000):
        raise UnsupportedDevice('Measurement mode9 requires whole getter/setter and preserves other GLOBAL fields')
    if mode is None:mode=dict(after_config_count=len(events),step=0,state='in-progress',accesses=[])
    event=dict(operation=operation,address=address,word=word,old_word=old,cpu=core,
               tick=device.board.virtual_ticks,step=step)
    mode['accesses'].append(event);mode['step']+=1
    mode['state']='completed' if mode['step']==3 else 'in-progress';record['continuation_mode9']=mode
    if operation=='write':
        device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    device.board.record('Source9 whole mode9 getter/setter retained',**event,
        limits='Only bits16..18; passive mode0..3 and flag0/1, no reset/DMA/completion/IRQ')
    return True


def _retirement_validate(device,record):
    """Replay only this measurement's whole92CD04/92F6EC close.

    The earlier fresh_shutdown belongs to generation3 and is never replaced.
    Retained idle890=0 follows the already selected unmeasured stop law; this
    transaction creates no busy response, data, IRQ or firmware callback.
    """
    close=record.get('retirement')
    if close is None:return None
    sequence=device.FRESH_SHUTDOWN_SEQUENCE
    close_keys={'profile','step','state','activation','data','accesses','started_tick','completed_tick'}
    if (type(close) is not dict or not close_keys<=set(close) or set(close)-close_keys-{'control'}
            or close['profile']!=RETIREMENT_PROFILE
            or type(close['step']) is not int or not 1<=close['step']<=len(sequence)
            or close['state']!=('completed' if close['step']==len(sequence) else 'in-progress')
            or close['activation']!=source_anchor(device)
            or close['data']!=afs_measurement_data.anchor(device,9)
            or type(close['accesses']) is not list or len(close['accesses'])!=close['step']
            or type(close['started_tick']) is not int
            or not close['data']['acknowledged_tick']<=close['started_tick']<=device.board.virtual_ticks
            or close['started_tick']!=close['accesses'][0]['tick']
            or close['completed_tick']!=(close['accesses'][-1]['tick'] if close['step']==len(sequence) else None)):
        raise UnsupportedDevice('Measurement retirement lost its closed second data and exact driver ownership')
    words={device.BASE+offset:word for offset,word in device.FRESH_SHUTDOWN_PRIOR.items()}
    previous=close['started_tick']
    for step,event in enumerate(close['accesses']):
        kind,offset,word=sequence[step];address=device.BASE+offset
        if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                or (event['operation'],event['address'],event['word'],event['cpu'],event['step'])!=(kind,address,word,0,step)
                or type(event['old_word']) is not int or event['old_word']!=words[address]
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement retirement differs from whole92CD04/92F6EC CPU0 WORD order')
        words[address]=word;previous=event['tick']
    return words


def retirement_anchor(device):
    """Pure closed hardware ledger; a new arm checks its live IRQ route.

    Source10 can retain this immutable close after a later activation changes
    current registers/IRQ46. No source/history validator is called here.
    """
    record=device.rearm.get('measurement') if type(device.rearm) is dict else None
    close=record.get('retirement') if type(record) is dict else None
    if type(close) is not dict or close.get('state')!='completed':
        raise UnsupportedDevice('Measurement next initialization requires completed hardware retirement')
    _retirement_validate(device,record)
    control=_retirement_control_validate(device,record)
    return dict(profile=close['profile'],state='completed' if control is not None and control['state']=='completed'
                else 'closed-awaiting-control',completed_tick=close['completed_tick'],
                activation=deepcopy(close['activation']),data=deepcopy(close['data']),
                accesses_sha256=hp.digest(close['accesses']),control=deepcopy(control))


def _retirement_access(device,record,operation,core,address,value):
    close=record.get('retirement');sequence=device.FRESH_SHUTDOWN_SEQUENCE
    if close is not None and close['state']=='completed':return False
    if close is None:
        if (operation,address)!=('read',device.BASE+4):return False
        data=record.get('data')
        if type(data) is not dict or data.get('profile')!=afs_measurement_data.CONTINUATION_PROFILE:return False
        if (device.stop_profile!='stock-idle-stop-handshakes-unmeasured'
                or type(data) is not dict or data.get('profile')!=afs_measurement_data.CONTINUATION_PROFILE
                or data.get('state')!='acknowledged'
                or record.get('continuation_rotation',{}).get('state')!='completed'
                or record.get('continuation_mode9',{}).get('state')!='completed'
                or any(device.registers[device.BASE+offset]!=word for offset,word in device.FRESH_SHUTDOWN_PRIOR.items())
                or any(device.registers.get(device.BASE+offset)!=0 for offset in (0x984,0x990,0x9f8))):
            raise UnsupportedDevice('Measurement idle retirement requires closed second data, complete A bank and inactive DMA channels')
        close=dict(profile=RETIREMENT_PROFILE,step=0,state='in-progress',activation=source_anchor(device),
                   data=afs_measurement_data.anchor(device,9),accesses=[],
                   started_tick=device.board.virtual_ticks,completed_tick=None)
    step=close['step'];kind,offset,word=sequence[step]
    old=device.registers[address];actual=old if operation=='read' else value
    if (type(core) is not int or core!=0 or (operation,address,actual)!=(kind,device.BASE+offset,word)
            or type(actual) is not int):
        raise UnsupportedDevice('Measurement idle retirement requires its next exact92CD04/92F6EC access')
    event=dict(operation=operation,address=address,word=actual,old_word=old,cpu=core,
               tick=device.board.virtual_ticks,step=step)
    close['accesses'].append(event);close['step']+=1
    close['state']='completed' if close['step']==len(sequence) else 'in-progress'
    if close['state']=='completed':close['completed_tick']=device.board.virtual_ticks
    record['retirement']=close
    if operation=='write':
        device.registers[address]=actual
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(actual),old=hex(old)))
    device.board.record('Measurement whole idle retirement retained',**event,state=close['state'],
        limits='Unmeasured idle zero convention; no new data/IRQ/acknowledgment/callback or activation')
    return True


def _retirement_control_validate(device,record):
    close=record.get('retirement');control=close.get('control') if type(close) is dict else None
    if control is None:return None
    if (close.get('state')!='completed' or type(control) is not dict
            or set(control)!={'prior_word','current_word','step','state','requests','irq_absent_observed_tick'}
            or type(control['prior_word']) is not int or not device._global_valid(control['prior_word'])
            or type(control['requests']) is not list or not 1<=len(control['requests'])<=2
            or type(control['step']) is not int or not 1<=control['step']<=25
            or control['prior_word']!=record['continuation_mode9']['accesses'][-1]['word']
            or type(control['irq_absent_observed_tick']) is not int
            or not close['completed_tick']<=control['irq_absent_observed_tick']<=device.board.virtual_ticks):
        raise UnsupportedDevice('Measurement post-close controls require bounded D4-zero/FC-zero history')
    previous=control['irq_absent_observed_tick'];word=control['prior_word'];total=0
    for index,request in enumerate(control['requests']):
        family=('D4-zero','FC-zero')[index]
        sequence=device._post_shutdown_sequence(word,family)
        if (type(request) is not dict or set(request)!={'family','step','state','prior_word','current_word','accesses'}
                or request['family']!=family or request['prior_word']!=word
                or type(request['step']) is not int or not 1<=request['step']<=len(sequence)
                or request['state']!=('completed' if request['step']==len(sequence) else 'in-progress')
                or index<len(control['requests'])-1 and request['state']!='completed'
                or type(request['accesses']) is not list or len(request['accesses'])!=request['step']):
            raise UnsupportedDevice('Measurement post-close request changed its whole10CA7F8 zero-disable family')
        for step,event in enumerate(request['accesses']):
            kind,value=sequence[step]
            if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                    or (event['operation'],event['address'],event['word'],event['cpu'],event['step'])
                        !=(kind,device.GLOBAL,value,0,step)
                    or type(event['old_word']) is not int or event['old_word']!=word
                    or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
                raise UnsupportedDevice('Measurement post-close GLOBAL getter/setter order changed')
            word=value;previous=event['tick'];total+=1
        if request['current_word']!=word:
            raise UnsupportedDevice('Measurement post-close request projection changed')
    if (control['step']!=total or control['current_word']!=word
            or control['state']!=control['requests'][-1]['state']):
        raise UnsupportedDevice('Measurement post-close control projection changed')
    return control


def _retirement_control_access(device,record,operation,core,address,value):
    close=record.get('retirement')
    if address!=device.GLOBAL or close is None or close['state']!='completed':return False
    control=close.get('control')
    if control is not None and control['state']=='completed' and len(control['requests'])==2:
        if operation=='read':return False
        raise UnsupportedDevice('Further measurement controls require a separate actual initializer')
    if any(bank.enabled&(1<<46) or bank.pending&(1<<46) or bank.active==46 or bank.injected==46
           for bank in device.board.interrupts.banks):
        raise UnsupportedDevice('Measurement post-close GLOBAL controls require real IRQ46 unregister/EOI')
    old=device.registers[address];word=old if operation=='read' else value
    if control is None:
        control=dict(prior_word=old,current_word=old,step=0,state='in-progress',requests=[],
                     irq_absent_observed_tick=device.board.virtual_ticks)
    requests=control['requests'];request=requests[-1] if requests and requests[-1]['state']!='completed' else None
    if request is None:
        family=('D4-zero','FC-zero')[len(requests)]
        request=dict(family=family,prior_word=old,current_word=old,step=0,state='in-progress',accesses=[])
    sequence=device._post_shutdown_sequence(request['prior_word'],request['family']);step=request['step']
    if type(core) is not int or core!=0 or type(word) is not int or (operation,word)!=sequence[step]:
        raise UnsupportedDevice('Measurement post-close requires next whole D4-zero then FC-zero getter/setter')
    if not requests or requests[-1] is not request:requests.append(request)
    event=dict(operation=operation,address=address,word=word,old_word=old,cpu=core,
               tick=device.board.virtual_ticks,step=step)
    request['accesses'].append(event);request['step']+=1;request['current_word']=word
    request['state']='completed' if request['step']==len(sequence) else 'in-progress'
    control['step']+=1;control['current_word']=word;control['state']=request['state'];close['control']=control
    if operation=='write':
        device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    device.board.record('Measurement post-close desired control retained',**event,family=request['family'],
        limits='At most one complete D4-zero and one FC-zero, including stock XOR; no reset/DMA/data/IRQ/newactivation')
    return True


def _late_access(device,record,operation,core,address,value):
    request=record['request']
    if request is None or request['step']!=22 or address not in device.KNOWN_DESIRED_CONFIG:return False
    if record.get('data',{}).get('profile')==afs_measurement_data.CONTINUATION_PROFILE:
        if address==device.GLOBAL:return _continuation_mode_access(device,record,operation,core,address,value)
        if operation=='read':return True
        return _continuation_access(device,record,operation,core,address,value)
    if operation=='read':return True
    rotation=record.get('rotation');events=record.get('late_configuration',[])
    old=device.registers[address]
    if (rotation is None or rotation['state']!='completed' or len(events)>=CONFIG_LIMIT
            or type(value) is not int or not device._known_pair_valid(address,old,value)):
        raise UnsupportedDevice('Measurement next-slot fields require the real source rotation and known masked setter')
    event=dict(operation=operation,address=address,word=value,old_word=old,cpu=core,tick=device.board.virtual_ticks,step=len(events))
    events.append(event);record['late_configuration']=events;device.registers[address]=value
    device.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
    device.board.record('Measurement next-slot desired field retained',**event,
                        limits='Current desired word only; initial activation and in-flight data remain frozen')
    return True


def _input_hash(marker):
    from .afs_statistics_frame import _input_hash
    return _input_hash(marker)


def _history_view(device, record):
    previous = record['previous']
    if (type(previous) is not dict or set(previous) != {'rearm', 'registers'}
            or hp.digest(previous) != record['previous_sha256']
            or type(previous['rearm']) is not dict or 'measurement' in previous['rearm']
            or type(previous['registers']) is not dict):
        raise UnsupportedDevice('Measurement producer immutable retired history changed')
    view = copy(device)
    view.registers = dict(previous['registers'])
    view.rearm = previous['rearm']
    view.board = copy(device.board)
    view.board.interrupts = copy(device.board.interrupts)
    view.board.interrupts.banks = [copy(bank) for bank in device.board.interrupts.banks]
    view.board.devices = []
    for item in device.board.devices:
        proxy = view if item is device else copy(item)
        proxy.board = view.board
        if type(item).__name__=='AfsFrameSource' and item.request is not None:
            # Validate the sealed generation3 lifetime against the immutable
            # source prefix which actually belonged to it. Later frame7/8
            # identities belong to this new measurement, not its retired arm.
            history=item.request.get('history',[])
            closed=[request for request in history if request.get('frame')==6]
            if len(closed)==1:
                proxy.request=deepcopy(closed[0])
                first=[request for request in history if request.get('frame')==5]
                if len(first)!=1:raise UnsupportedDevice('Measurement archived source prefix changed')
                proxy.request['history']=[deepcopy(first[0])]
                proxy.request['history_sha256']=hp.digest(proxy.request['history'])
        view.board.devices.append(proxy)
    # Its close really unregistered this IRQ. A new registration belongs to
    # the new lifetime; copied validation must preserve the historical fact.
    for bank in view.board.interrupts.banks:
        bank.enabled &= ~(1 << 46)
        bank.pending &= ~(1 << 46)
        if bank.active == 46:bank.active = None
        if bank.injected == 46:bank.injected = None
    view._validate_rearm()
    return view


def _idle_validate(device,record):
    idle=record.get('idle_stop')
    if idle is None:return
    reset=record['reset'];sequence=device.PREVIEW_IDLE_STOP
    if (type(idle) is not dict or set(idle) != {'step','state','accesses'}
            or type(idle['step']) is not int or not 1 <= idle['step'] <= len(sequence)
            or idle['state'] != ('completed' if idle['step']==len(sequence) else 'in-progress')
            or type(idle['accesses']) is not list or len(idle['accesses']) != idle['step']
            or reset is None or reset['state'] != 'completed'):
        raise UnsupportedDevice('Measurement idle stop requires completed reset and complete stock channel sequence')
    previous=reset['accesses'][-1]['tick']
    for step,event in enumerate(idle['accesses']):
        kind,offset,word=sequence[step]
        if (type(event) is not dict or set(event) != {'operation','address','word','cpu','tick','step'}
                or (event['operation'],event['address'],event['word'],event['cpu'],event['step'])
                  != (kind,device.BASE+offset,word,0,step)
                or type(event['tick']) is not int or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Measurement idle stop differs from whole92CC5C CPU0 WORD order')
        previous=event['tick']


def _idle_access(device,record,operation,core,address,size,value):
    idle=record.get('idle_stop');sequence=device.PREVIEW_IDLE_STOP
    if idle is None:
        if (operation,address,value) != ('write',device.BASE+sequence[0][1],0):return False
        if record['reset'] is None or record['reset']['state']!='completed' or record['request'] is not None:
            raise UnsupportedDevice('Measurement idle stop requires completed stock GLOBAL reset')
        idle=dict(step=0,state='in-progress',accesses=[])
    elif idle['state']=='completed':return False
    step=idle['step'];kind,offset,expected=sequence[step]
    word=device.registers[address] if operation=='read' else value
    if (operation,address,word) != (kind,device.BASE+offset,expected):
        raise UnsupportedDevice('Measurement idle stop requires next complete92CC5C access')
    event=dict(operation=operation,address=address,word=word,cpu=core,tick=device.board.virtual_ticks,step=step)
    idle['accesses'].append(event);idle['step']+=1
    idle['state']='completed' if idle['step']==len(sequence) else 'in-progress'
    if operation=='write':
        old=device.registers[address];device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    record['idle_stop']=idle
    device.board.record('Measurement producer whole idle stop retained',**event,state=idle['state'],
        limits='Known unmeasured already-idle zero convention; no data/IRQ/callback')
    return True


def arm(device, profile=PROFILE):
    if (type(profile) is not str or profile != PROFILE or type(device.rearm) is not dict
            or 'measurement' in device.rearm):
        raise UnsupportedDevice('Unknown or repeated generation4 measurement admission')
    device._validate_rearm()
    marker = _marker(device)
    data = device.rearm.get('afs_statistics')
    shutdown, controls = device.fresh_shutdown, device.post_shutdown_control
    if (marker['state'] != 'waiting' or not device.board.virtual_ticks < marker['due_tick']
            or type(data) is not dict or (data.get('lifetime'), data.get('state')) != (2, 'acknowledged')
            or type(shutdown) is not dict or shutdown.get('state') != 'completed'
            or type(controls) is not dict or controls.get('state') != 'completed'
            or [request.get('family') for request in controls['requests']] != ['D4-zero', 'FC-zero']
            or device.preview_configuration is not None or device.preview_idle_stop is not None
            or device.busy or device.dma_busy
            or any(value is not None for value in (device.pending_stop,device.pending_data,device.pending_ack))
            or any(bank.enabled & (1 << 46) for bank in device.board.interrupts.banks)
            or any(device.registers[device.BASE+offset] != word for offset,word in
                   ((4,0x33170000),(8,0x10000),(0x884,0),(0x890,0),(0x8f8,0)))
            or device.registers[device.GLOBAL] != 0x10030000):
        raise UnsupportedDevice('Measurement admission requires both consumed frames and actual producer retirement')
    previous = dict(rearm=deepcopy(device.rearm), registers=dict(device.registers))
    record = dict(profile=profile, cpu=0, armed_tick=device.board.virtual_ticks,
                  input_sha256=_input_hash(marker), previous=previous,
                  previous_sha256=hp.digest(previous), accesses=[], prior_words={}, current_words={},
                  request=None,reset=None,idle_stop=None)
    device.rearm['measurement'] = record
    device.board.record('Generation4 measurement producer explicitly admitted',profile=profile,
                        limits='One future real descriptor/arm; admission has no DMA/status/IRQ/guest effects')
    return deepcopy(record)


def records(device):
    record = device.rearm
    if type(record) is not dict:return []
    measurement = record.get('measurement')
    fields=('descriptor_step','setup_step','enable_step','descriptor_accesses','setup_accesses','enable_accesses',
            'started_tick','armed_tick','enabled_tick','route','original_history_sha256','state')
    if measurement is None:return [deepcopy({key:record[key] for key in fields})]
    if hp.digest(measurement['previous']) != measurement['previous_sha256']:
        raise UnsupportedDevice('Measurement producer archived anchor changed')
    previous=measurement['previous']['rearm']
    return [deepcopy({key:previous[key] for key in fields})]


def validate(device,*,check_history=True):
    record = device.rearm.get('measurement') if type(device.rearm) is dict else None
    if record is None:return None
    base_keys={'profile','cpu','armed_tick','input_sha256','previous','previous_sha256',
               'accesses','prior_words','current_words','request','reset'}
    if (type(record) is not dict or not base_keys<=set(record)
            or set(record)-base_keys-{'idle_stop','data','rotation','late_configuration',
                                  'data_history','data_history_sha256','continuation_rotation','continuation_configuration',
                                  'continuation_mode9','retirement'}
            or record.get('profile') != PROFILE or type(record.get('cpu')) is not int or record['cpu'] != 0
            or type(record.get('armed_tick')) is not int or not 0 <= record['armed_tick'] <= device.board.virtual_ticks
            or type(record.get('accesses')) is not list or len(record['accesses']) > CONFIG_LIMIT
            or type(record.get('prior_words')) is not dict or type(record.get('current_words')) is not dict
            or device.busy is not False or device.dma_busy is not False
            or any(value is not None for value in (device.pending_data,device.pending_ack,device.pending_stop))):
        raise UnsupportedDevice('Measurement producer admission/plain fields changed')
    marker = _marker(device,full=check_history)
    if _input_hash(marker) != record['input_sha256']:
        raise UnsupportedDevice('Measurement producer source identity changed')
    if check_history:_history_view(device,record)
    elif (type(record['previous']) is not dict or set(record['previous'])!={'rearm','registers'}
            or type(record['previous']['rearm']) is not dict or type(record['previous']['registers']) is not dict):
        raise UnsupportedDevice('Measurement producer retired projection shape changed')
    reset_word=_reset_validate(device,record)
    _idle_validate(device,record)
    words = dict(record['previous']['registers']); prior = {}; current = {}
    request=record['request']
    frozen_mmio=(not check_history and type(request) is dict and request.get('step')==22
                 and len(_source_records(device.board,full=False))>=4)
    if frozen_mmio:
        # Source8 sealed these desired words and the actual22-access
        # activation before its pixel edge. Audit that small anchor on each
        # MMIO instead of replaying1700 retired initialization accesses.
        anchor=source_anchor(device)
        pixels=[item for item in _source_records(device.board,full=False) if item['frame']>=8]
        if (any(item.get('measurement_producer')!=anchor for item in pixels)
                or set(record['prior_words'])!=set(record['current_words'])
                or any(type(address) is not int or address not in device.KNOWN_DESIRED_CONFIG
                    or type(word) is not int or not 0<=word<1<<32
                    for address,word in record['current_words'].items())):
            raise UnsupportedDevice('Measurement frozen initial words lost their physical activation anchor')
        words.update(record['current_words'])
    for step,event in enumerate(() if frozen_mmio else record['accesses']):
        if reset_word is not None and step==record['reset']['after_access_count']:
            if record['reset']['state']!='completed':
                raise UnsupportedDevice('Measurement configuration precedes completed GLOBAL reset')
            words[device.GLOBAL]=reset_word
        if (type(event) is not dict or set(event) != {'operation','address','word','old_word','cpu','tick','step'}
                or type(event['address']) is not int or event['address'] not in device.KNOWN_DESIRED_CONFIG
                or type(event['cpu']) is not int or event['cpu'] != 0 or type(event['step']) is not int or event['step'] != step
                or type(event['tick']) is not int or not record['armed_tick'] <= event['tick'] <= device.board.virtual_ticks
                or step and event['tick'] < record['accesses'][step-1]['tick']
                or type(event['old_word']) is not int or event['old_word'] != words[event['address']]
                or type(event['word']) is not int or event['operation'] not in ('read','write')
                or event['operation'] == 'read' and (event['word'] != event['old_word'] or event['address'] not in device.READABLE)
                or event['operation'] == 'write' and not device._known_pair_valid(event['address'],event['old_word'],event['word'])):
            raise UnsupportedDevice('Measurement producer desired-field transaction changed')
        prior.setdefault(event['address'],event['old_word']);current[event['address']]=event['word']
        words[event['address']] = event['word']
    if not frozen_mmio and (record['prior_words'] != prior or record['current_words'] != current):
        raise UnsupportedDevice('Measurement producer desired-field projection changed')
    if reset_word is not None and len(record['accesses'])==record['reset']['after_access_count']:
        words[device.GLOBAL]=reset_word
    request = record['request']
    if request is not None:
        sequence = device._preview_request_sequence()
        if (type(request) is not dict or set(request) != {'step','state','accesses','started_tick','armed_tick','enabled_tick','route'}
                or type(request['step']) is not int or not 1 <= request['step'] <= len(sequence)
                or request['state'] != device._preview_request_state(request['step'])
                or type(request['accesses']) is not list or len(request['accesses']) != request['step']
                or request['started_tick'] != request['accesses'][0]['tick']):
            raise UnsupportedDevice('Measurement producer activation phase changed')
        previous_tick = record['armed_tick']
        for step,event in enumerate(request['accesses']):
            kind,address,value = sequence[step]
            if (type(event) is not dict or set(event) != {'operation','address','word','old_word','cpu','tick','step'}
                    or (event['operation'],event['address'],event['cpu'],event['step']) != (kind,address,0,step)
                    or type(event['word']) is not int or type(event['old_word']) is not int
                    or event['old_word'] != words[address]
                    or (not device._config_valid(address,event['word']) if value is None else event['word'] != value)
                    or type(event['tick']) is not int or not previous_tick <= event['tick'] <= device.board.virtual_ticks):
                raise UnsupportedDevice('Measurement producer exact activation transaction changed')
            words[address]=event['word'];previous_tick=event['tick']
        if (request['armed_tick'] != (request['accesses'][17]['tick'] if request['step'] >= 18 else None)
                or request['enabled_tick'] != (request['accesses'][21]['tick'] if request['step'] == 22 else None)):
            raise UnsupportedDevice('Measurement producer activation timing changed')
        data=record.get('data')
        # The activation registration remains immutable while its own real
        # data IRQ is pending/active. Numeric delivery and the paired ACK
        # separately validate that ownership and its current peripheral phase.
        allow_inflight=type(data) is dict and data.get('state') in ('delivered','acknowledged')
        closed=record.get('retirement',{}).get('state')=='completed'
        no_route=closed and all(not bank.enabled&(1<<46) and not bank.pending&(1<<46)
                               and bank.active!=46 and bank.injected!=46 for bank in device.board.interrupts.banks)
        route=(dict(cpu=0,vector=0x92c8cc,priority=13,source_configuration=1) if no_route
               else device._preview_request_route(0,request['step'] >= 16,allow_inflight=allow_inflight))
        if request['route'] != (route if request['step'] >= 16 else None) or request['step'] < 15 and route is not None:
            raise UnsupportedDevice('Measurement producer activation IRQ registration order changed')
    _rotation_validate(device,record,words,full_source=check_history)
    _late_validate(device,record,words)
    _rotation_validate(device,record,words,key='continuation_rotation',frame=9,base=0x5fecaa60,
                       full_source=check_history)
    _continuation_validate(device,record,words)
    _continuation_mode_validate(device,record,words)
    if check_history:afs_measurement_data.validate_history(device,record)
    data=afs_measurement_data.validate(device,record,full_source=check_history)
    if data is not None:words.update(data['ack_words'])
    closed_words=_retirement_validate(device,record)
    if closed_words is not None:words.update(closed_words)
    control=_retirement_control_validate(device,record)
    if control is not None:words[device.GLOBAL]=control['current_word']
    if device.registers != words:
        raise UnsupportedDevice('Measurement producer live registers differ from its retired/desired/activation ledgers')
    return record


def access(device,operation,core,address,size,value=None):
    record = validate(device,check_history=False)
    if record is None:return False
    if type(core) is not int or core != 0 or type(size) is not int or size != 4:
        raise UnsupportedDevice('Measurement producer requires CPU0 WORD accesses')
    if _reset_access(device,record,operation,core,address,size,value):return True
    if _idle_access(device,record,operation,core,address,size,value):return True
    if afs_measurement_data.access(device,record,operation,core,address,size,value):return True
    if _retirement_access(device,record,operation,core,address,value):return True
    if _retirement_control_access(device,record,operation,core,address,value):return True
    if _rotation_access(device,record,operation,core,address,value):return True
    if _late_access(device,record,operation,core,address,value):return True
    request = record['request']
    old = device.registers[address];word = old if operation == 'read' else value
    event = dict(operation=operation,address=address,word=word,old_word=old,cpu=core,tick=device.board.virtual_ticks)
    if request is None and address in device.KNOWN_DESIRED_CONFIG:
        if (len(record['accesses']) >= CONFIG_LIMIT or type(word) is not int
                or operation == 'read' and address not in device.READABLE
                or operation == 'write' and not device._known_pair_valid(address,old,word)):
            raise UnsupportedDevice('Measurement producer excludes unaudited desired fields')
        event['step']=len(record['accesses'])
        record['prior_words'].setdefault(address,old);record['current_words'][address]=word
        record['accesses'].append(event)
    elif request is not None and request['step'] == 22 and operation == 'read' and address in device.KNOWN_DESIRED_CONFIG:
        return True
    else:
        sequence = device._preview_request_sequence();step = 0 if request is None else request['step']
        if step >= len(sequence):
            raise UnsupportedDevice('Measurement numeric output/next acquisition requires a separate explicit data law')
        kind,target,expected = sequence[step]
        if (operation != kind or address != target or type(word) is not int
                or (not device._config_valid(address,word) if expected is None else word != expected)):
            raise UnsupportedDevice('Measurement producer requires its next exact descriptor/setup/enable access')
        marker = _marker(device,full=False)
        if request is None:
            if marker['state'] not in ('asserted','acknowledged') or marker['asserted_tick'] > device.board.virtual_ticks:
                raise UnsupportedDevice('Measurement producer activation requires its real source edge')
            request=dict(step=0,state='descriptor-rebuild',accesses=[],started_tick=device.board.virtual_ticks,
                         armed_tick=None,enabled_tick=None,route=None)
        route=device._preview_request_route(0,step >= 15)
        if step < 15 and route is not None:
            raise UnsupportedDevice('Measurement producer IRQ registration precedes its real initialization')
        event['step']=step;request['accesses'].append(event);request['step']+=1
        request['state']=device._preview_request_state(request['step'])
        if request['step'] >= 16:request['route']=route
        if request['step'] == 18:request['armed_tick']=device.board.virtual_ticks
        if request['step'] == 22:request['enabled_tick']=device.board.virtual_ticks
        record['request']=request
    if operation == 'write':
        device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
    device.board.record('Measurement producer transaction retained',**event,
                        limits='Desired fields only; separate numeric data law required')
    return True
