"""Two stock image-output banks with an optional declared virtual data source.

The unchanged92A8D0 driver clears control bits0/16 and polls bit0 SET on
FFF12000 thenFFF13000 before writing D4=33FF1. An independent opt-in supplies
that initial, inactive-bank acknowledgment after an unmeasured delay. The
seven setter families retain geometry/control/destination words; D8=1 retains
registration and controlbit16 retains a requested arm awaiting input. Nothing
produces data by default. An independent opt-in samples two candidateYYCbCr
tuples from peripheral source edges, writes Rust-generated payloads before
IRQs and accepts their actual D4 acknowledgments. Timing, units and latching
are declared experiment conventions, not recovered silicon behavior.
An independent stop profile acknowledges one idle active stop per bank after
all modeled image transfers and D4 completions drain, then retains the exact
stock passive reset recipe. Unknown or in-flight shutdown remains unsupported.
An explicit AF-S admission permits one additional current-generation2 pair
for a real source5/6 identity. It retains the original two jobs and runs the
same payload, IRQ and D4 driver laws; admission/source timing is unmeasured.
An explicit contrast initializer may retain one new D8/read/control arm per
bank after the completed mode reset, without producing another image job.
An independent first-pixel opt-in latches those desired outputs before
Source0, resamples the acknowledged supplied YYCbCr scene and emits the
paired image IRQs only after both payloads exist. Its DMA/ACK lifetime stays
separate from the original three jobs and later desired geometry stores.
"""
import hashlib
import json
import struct
from copy import deepcopy

from . import strict_audit
from .hardware import UnsupportedDevice
from .interrupts import IrqRoute
from .virtual_source import source_epochs, epoch_key, ram_span
from .image_stream import ImageStreamProfile


STOP_PROFILE = 'stock-initial-image-banks-bit0-stop-ack-unmeasured'
IDLE_STOP_PROFILE = 'stock-idle-image-banks-bit0-stop-ack-unmeasured'
STOP_PROFILES = (STOP_PROFILE, IDLE_STOP_PROFILE)
DATA_PROFILE = 'declared-postisp-yycbcr-scene-unmeasured'
DATA_PROFILES = (DATA_PROFILE, *ImageStreamProfile)
AFS_DATA_PROFILE = 'declared-afs-latched-postisp-pair-unmeasured'
AFS_REARM_PROFILE = 'declared-afs-contrast-image-rearm-after-mode-reset-unmeasured'
AFS_PREVIEW_DATA_PROFILE = 'declared-afs-first-preview-yycbcr-source0-latched-unmeasured'
AFS_PREVIEW_STOP_PROFILE = 'declared-afs-drained-preview-bank-stop-unmeasured'


class ImageWorkControl:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    stream=None  # Plain state of the opt-in continuous image stream.
    BASES = (0xfff12000, 0xfff13000)
    CONFIG_MASKS = {0x04:0x0fff0fff, 0x08:0x8000ffff, 0x0c:0x0000ffff,
        0x14:0x1fff1fff, 0xb0:0x07ff07ff, 0xb4:0x07ff07ff,
        0xb8:0x01ff0037, 0xc0:0x3ffffff0, 0xc4:0x00001ff8, 0xf4:0x0000000f}
    CONFIG_ORDER = (0x04,0x08,0x0c,0x14,0xb0,0xb4,0xb8,0xc0,0xc4,0xf4)
    # 10CBB44/92AA90 repeat desired tuples; 92AA9C reads retained dimensions without polling or a new DMA/IRQ.
    ACTIVE_CONFIG_LIMIT = 256 * len(CONFIG_ORDER)
    #10CB618 and10CA754 retain initial/reset configuration only. Neither
    #writes image control or supplies a completion/status response.
    PASSIVE_CONFIG_MASKS = {0x10:0x1fff1fff,0x18:0x1fff1fff,0x1c:0x7fff7fff,
        0x20:0x003f000f,**{o:0x1ffff for o in range(0x30,0xac,4)}}
    PASSIVE_CONFIG_ORDER = (0x10,0x18,0x1c,0x20,*range(0x30,0xac,4))
    PASSIVE_CONFIG_LIMIT = 256
    IDLE_RESET_SEQUENCE = (('write',0x10),('read',0x1c),('read',0x20),
        ('write',0x1c),('write',0x20),*(('write',o) for o in range(0x30,0xac,4)))
    ALL_CONFIG_MASKS = {**CONFIG_MASKS,**PASSIVE_CONFIG_MASKS}
    READABLE = frozenset((0x00,*ALL_CONFIG_MASKS))
    WRITABLE = frozenset((0x00,0xd4,0xd8,*ALL_CONFIG_MASKS))
    STOP_ACK = 0x33ff1
    CLOSED_RESET_POLL_LIMIT = 65536
    IRQ_CONTRACT = ((0x2f,0x2c,0x92a71c),(0x30,0x2d,0x92a74c))

    def __init__(self, board, stop_profile=None, stop_ticks=2, data_profile=None, data_ticks=400000):
        if stop_profile is not None and stop_profile not in STOP_PROFILES:
            raise ValueError('Unknown image-bank stop profile')
        if type(stop_ticks) is not int or stop_ticks<1:
            raise ValueError('Image-bank stop delay must be a positive integer')
        if data_profile is not None and data_profile not in DATA_PROFILES:
            raise ValueError('Unknown image virtual data profile')
        if data_profile == ImageStreamProfile.XM1.value:
            data_profile = ImageStreamProfile.XM1
        if type(data_ticks) is not int or data_ticks < 1:
            raise ValueError('Image virtual data delay must be a positive integer')
        self.board=board
        self.stop_profile=stop_profile
        self.stop_ticks=stop_ticks
        self.data_profile,self.data_ticks=data_profile,data_ticks
        self.pending_data=None
        self.data_history=[]
        self.dma_busy=False
        self.registers={base+off:0 for base in self.BASES
            for off in (0x00,0xd4,0xd8,*self.ALL_CONFIG_MASKS)}
        self.banks=[dict(state='initial-inactive-fixture',pending_stop=None,last_stop=None,
            config_step=0,configuration_writes=[],registration=None,arm=None,
            passive_configuration_writes=[],passive_prior_words={},passive_started=False,
            passive_stores=0,passive_ledger_sha256=None,
            active_config_step=0,active_generation=0,active_configuration_writes=[],
            pending_completion=None,
            stop_requests=0,stop_completions=0,stop_acknowledgments=0) for _ in self.BASES]
        self.busy=False
        self.writes=[]
        self.last_rejected=None
        board.record('Image work banks initial fixture',bases=[hex(a) for a in self.BASES],
            stop_profile=stop_profile,stop_ticks=stop_ticks,
            evidence='Frozen225 original-byte audit:92A8D0 stop,10CBB44 seven setter families,92B978 registration,92A9AC desired arm',
            limits='Zero initial configuration/inactive controls and optional bit0 acknowledgment latency/meaning are unmeasured; no DMA,image,status event,IRQ,callback,metadata,pixels or completion')
        if type(data_profile) is ImageStreamProfile:
            self.enable_stream(profile=data_profile, data_delay_ticks=data_ticks)

    def contains(self, address, size):
        return any(base<=address<base+0x100 for base in self.BASES)

    def _reject(self,operation,core,address,size,value=None,reason='Unknown image-bank operation'):
        self.last_rejected=dict(operation=operation,cpu=core,address=hex(address),size=size,
            value=hex(value) if type(value) is int else repr(value),reason=reason)
        self.board.record('Image work bank operation rejected',**self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    def _offset(self,operation,core,address,size,value=None):
        for channel,base in enumerate(self.BASES):
            if base<=address<base+0x100:
                offset=address-base
                allowed=self.READABLE if operation=='read' else self.WRITABLE
                if size!=4 or offset not in allowed:
                    self._reject(operation,core,address,size,value,
                        reason='Unknown image-bank offset or non-WORD access')
                return channel,offset
        self._reject(operation,core,address,size,value,reason='Address outside image banks')

    @classmethod
    def _config_valid(cls, offset, value):
        if type(value) is not int or not 0<=value<=0xffffffff or value&~cls.ALL_CONFIG_MASKS[offset]:
            return False
        if offset==0xb0:
            return (value&0x7ff)>=7 and (value&7)==7
        if offset==0xb4:
            return (value&0x7ff)>=7 and (value&7)==7 and ((value>>16)&7)==0
        if offset==0xb8:
            return (value&7)<=4
        if offset==0x20:
            return (value&0xf)<=8 and ((value>>16)&0x3f)<=0x20
        return True

    def _config_hash(self, channel):
        base=self.BASES[channel]
        if any(type(self.registers[base+off]) is not int
                or not 0<=self.registers[base+off]<=0xffffffff for off in self.ALL_CONFIG_MASKS):
            raise UnsupportedDevice('Image-bank configuration word has unknown type or range')
        return hashlib.sha256(b''.join(struct.pack('<II',base+off,self.registers[base+off])
            for off in sorted(self.ALL_CONFIG_MASKS))).hexdigest()

    @staticmethod
    def _passive_hash(events):
        return hashlib.sha256(b''.join(struct.pack('<5IQ',event['step'],event['cpu'],
            int(event['address'],16),event['old_word'],event['word'],event['tick'])
            for event in events)).hexdigest()

    def _validate_passive(self,channel):
        bank=self.banks[channel];base=self.BASES[channel]
        events=bank['passive_configuration_writes'];prior=bank['passive_prior_words']
        stores=bank['passive_stores']
        if (not isinstance(events,list) or not isinstance(prior,dict) or type(stores) is not int
                or not 0<=stores<=self.PASSIVE_CONFIG_LIMIT or len(events)!=stores
                or type(bank['passive_started']) is not bool or bank['passive_started']!=(stores>0)):
            raise UnsupportedDevice('Image-bank passive configuration marker/count/history changed')
        current={};previous_tick=0
        for step,event in enumerate(events):
            if not isinstance(event,dict) or not isinstance(event.get('address'),str):
                raise UnsupportedDevice('Image-bank passive configuration access is not a plain typed record')
            try:address=int(event['address'],16)
            except ValueError as error:
                raise UnsupportedDevice('Image-bank passive configuration address is malformed') from error
            offset=address-base
            if (offset not in self.PASSIVE_CONFIG_MASKS or event['address']!=hex(address)
                    or event.get('operation')!='write' or type(event.get('step')) is not int or event['step']!=step
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                    or type(event.get('tick')) is not int or not previous_tick<=event['tick']<=0xffffffffffffffff
                    or not self._config_valid(offset,event.get('word'))
                    or not self._config_valid(offset,event.get('old_word'))
                    or event.get('value')!=hex(event['word']) or event.get('old')!=hex(event['old_word'])
                    or event['old_word']!=current.get(hex(address),prior.get(hex(address)))):
                raise UnsupportedDevice('Image-bank passive configuration store/prior chain changed')
            current[hex(address)]=event['word'];previous_tick=event['tick']
        # A later closed reset owns its current register projection separately.
        # The preceding passive ledger remains an immutable historical prefix.
        closed=self.banks[0].get('closed_reset')
        if closed is not None and (type(closed) is not dict
                or type(closed.get('prior_words')) is not dict
                or set(closed['prior_words'])!=set(self.registers)):
            raise UnsupportedDevice('Closed image reset historical register projection changed')
        registers=closed['prior_words'] if closed is not None else self.registers
        if (set(prior)!=set(current)
                or any(not self._config_valid(int(a,16)-base,prior[a]) for a in current)
                or any(type(registers[int(a,16)]) is not int or registers[int(a,16)]!=w
                    for a,w in current.items())
                or bank['passive_ledger_sha256']!=(self._passive_hash(events) if stores else None)):
            raise UnsupportedDevice('Image-bank passive configuration current words or ledger hash changed')

    def _idle_quiescent(self, *, historical=False):
        """Model-owned transfer/ACK drain, without consulting firmware RAM."""
        preview=self._afs_preview_marker()
        if (self.pending_data is not None
                or not historical and self.dma_busy
                or not historical and preview is not None and preview['state']!='acknowledged'
                or any(b['pending_completion'] is not None or b['active_config_step']
                       for b in self.banks)
                or any(job.get('state')!='acknowledged' or len(job.get('outputs',()))!=2
                       or any('acknowledged_tick' not in item for item in job['outputs'])
                       for job in self.data_history)):
            raise UnsupportedDevice('Idle image-bank stop requires all modeled DMA/completions/configuration drained')

    def _idle_reset_access(self,operation,core,channel,offset,value=None):
        bank=self.banks[channel]
        if bank['state']!='active-idle-stopped':return False
        step=bank['idle_reset_step']
        if (self.stop_profile!=IDLE_STOP_PROFILE or bank['last_stop']['state']!='poll-observed'
                or type(core) is not int or core!=bank['last_stop']['cpu']
                or step>=len(self.IDLE_RESET_SEQUENCE)
                or (operation,offset)!=self.IDLE_RESET_SEQUENCE[step]):
            self._reject(operation,core,self.BASES[channel]+offset,4,value,
                reason='Idle image-bank reset requires its bounded stock write/read sequence and owner')
        if operation=='write':
            expected=(0x10001000 if offset==0x10 else 0 if offset==0x1c else
                      self.registers[self.BASES[channel]+0x20] if offset==0x20 else 0x10000)
            if value!=expected:
                self._reject(operation,core,self.BASES[channel]+offset,4,value,
                    reason='Idle image-bank reset word differs from its stock retained-enum recipe')
        bank['idle_reset_step']+=1
        if bank['idle_reset_step']==len(self.IDLE_RESET_SEQUENCE):
            bank['idle_reset_completed_tick']=self.board.virtual_ticks
        return True

    def _closed_reset_history_hash(self):
        # Keep the legacy bank dictionaries, registrations and arm/config/data
        # records historical. The later paired reset lives inside one plain
        # nested ledger, so no constructor or portable object schema changes.
        banks=[{k:v for k,v in b.items() if k!='closed_reset'} for b in self.banks]
        return hashlib.sha256(json.dumps(dict(banks=banks,data=self.data_history),
            sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()

    def _closed_reset_ready(self,core):
        self._idle_quiescent(historical=True)
        preview=self._afs_preview_marker()
        pending=preview is not None and preview['state']=='pending'
        if (self.stop_profile!=IDLE_STOP_PROFILE
                or self.dma_busy is not pending or self.busy is not (pending or self._afs_stop_waiting())
                or type(core) is not int or core not in (0,1)
                or len({b.get('idle_reset_step') for b in self.banks})!=1
                or any(b['state']!='active-idle-stopped'
                    or b.get('idle_reset_step') not in (0,len(self.IDLE_RESET_SEQUENCE))
                    or b['last_stop']['state']!='poll-observed' or b['last_stop']['cpu']!=core
                    or b['arm'] is None or b['registration'] is None
                    or b['pending_stop'] is not None for b in self.banks)):
            raise UnsupportedDevice('Closed image reset requires both completed quiet closes and their owner')
        for channel in (0,1):self._validate_passive(channel)

    def _afs_rearm_clock(self,marker=None):
        from .afs_frame_source import af_requests
        records=af_requests(self.board)
        matches=[r for r in records if r.get('kind')=='control'
                 and r.get('control_phase')=='initialize-source0'
                 and (marker is None or (r.get('frame'),r.get('input_sha256'))==
                      (marker['frame'],marker['input_sha256']))]
        if len(matches)!=1 or matches[0].get('canceled_tick') is not None:
            raise UnsupportedDevice('AF image rearm requires its unique declared initializer source0')
        return matches[0]

    def _afs_rearm_route(self,channel,enabled):
        controller=getattr(self.board,'interrupts',None)
        if controller is None or len(controller.banks)!=2:
            raise UnsupportedDevice('AF image rearm requires its real kernel interrupt controller')
        _,line,vector=self.IRQ_CONTRACT[channel]
        if any(b.pending&(1<<line) or b.active==line or b.injected==line for b in controller.banks):
            raise UnsupportedDevice('AF image rearm requires its old completion IRQ drained')
        if (bool(controller.banks[0].enabled&(1<<line))!=enabled
                or controller.banks[1].enabled&(1<<line)
                or enabled and (controller.banks[0].config.get(0x400+4*line)!=vector
                    or controller.banks[0].config.get(0x800+4*line)!=10
                    or not controller.banks[0].source_config&(1<<line))):
            raise UnsupportedDevice('AF image rearm lacks its exact retired or newly registered CPU0 route')

    def arm_afs_rearm(self,profile=AFS_REARM_PROFILE):
        """Select one future six-access stock arm; no input or DMA is supplied."""
        self._validate_afs_data();self._validate_closed_reset()
        record=self.banks[0].get('closed_reset')
        if (profile!=AFS_REARM_PROFILE or record is None or record['cpu']!=0
                or record.get('af_rearm') is not None or self.data_profile!=DATA_PROFILE
                or any(record.get(k,{}).get('state')!='complete' for k in
                       ('mode_configuration','passive_configuration','passive_reset'))):
            raise UnsupportedDevice('AF image rearm requires its completed paired mode stop/configuration/reset')
        clock=self._afs_rearm_clock();edge=clock.get('source0')
        read=lambda a,n=4:int.from_bytes(self.board.read_memory(a,n,cpu_id=0),'little')
        if (type(edge) is not dict or edge['state']!='waiting' or clock.get('consumed_tick') is not None
                or clock.get('phase_at_arm')!=11 or read(0x16223f8,1)!=11
                or read(0x1625f5c)!=5 or read(0x1625f4c)!=0 or read(0x16260bc)!=0
                or read(0x15f6e50)!=0
                or any(read(a)==0 for a in (0x160e538,0x160e570))
                or any(read(0xccc1f4+16*c)!=0 for c in (0,1))
                or [read(0x15f6574),read(0x15f6600)]!=[0x11f5e60,0x11f5e20]):
            raise UnsupportedDevice('AF image rearm caller differs from the actual AF11/IMGDRV5 initializer')
        for channel in (0,1):self._afs_rearm_route(channel,False)
        marker=dict(profile=profile,state='selected',frame=clock['frame'],input_sha256=clock['input_sha256'],
            selected_tick=self.board.virtual_ticks,configuration_sha256=[self._config_hash(c) for c in (0,1)],
            accesses=[],arms=[])
        record['af_rearm']=marker
        self.board.record('Declared AF image rearm selected',frame=marker['frame'],profile=profile,
            limits='One stock D8/read/control arm per bank only; no source, payload, event or IRQ')
        return deepcopy(marker)

    def _validate_afs_rearm(self,marker,words,previous_tick):
        if (type(marker) is not dict or set(marker)!=({'profile','state','frame','input_sha256',
                'selected_tick','configuration_sha256','accesses','arms'}
                | ({'refresh'} if 'refresh' in marker else set())
                | ({'refresh_history'} if 'refresh_history' in marker else set())
                | ({'preview_data'} if 'preview_data' in marker else set())
                | ({'final_stop'} if 'final_stop' in marker else set()))
                or marker['profile']!=AFS_REARM_PROFILE or type(marker['frame']) is not int
                or type(marker['input_sha256']) is not str or len(marker['input_sha256'])!=64
                or type(marker['selected_tick']) is not int
                or not previous_tick<=marker['selected_tick']<=self.board.virtual_ticks
                or type(marker['accesses']) is not list or len(marker['accesses'])>6
                or type(marker['arms']) is not list):
            raise UnsupportedDevice('AF image rearm plain ownership/schema changed')
        self._afs_rearm_clock(marker)
        hashes=[hashlib.sha256(b''.join(struct.pack('<II',self.BASES[c]+o,words[self.BASES[c]+o])
            for o in sorted(self.ALL_CONFIG_MASKS))).hexdigest() for c in (0,1)]
        if marker['configuration_sha256']!=hashes:
            raise UnsupportedDevice('AF image rearm changed its completed desired configuration')
        arms=[];previous_tick=marker['selected_tick']
        for index,event in enumerate(marker['accesses']):
            channel,step=divmod(index,3);base=self.BASES[channel]
            expected=(('write',base+0xd8,1),('read',base,1),('write',base,0x10001))[step]
            if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                    or (event['operation'],event['address'],event['word'])!=expected
                    or type(event['cpu']) is not int or event['cpu']!=0
                    or type(event['step']) is not int or event['step']!=index
                    or type(event['tick']) is not int or not previous_tick<=event['tick']<=self.board.virtual_ticks
                    or words[base]!=1 or words[base+0xd8]!=1):
                raise UnsupportedDevice('AF image rearm differs from D8/read/desired-control driver order')
            if event['operation']=='write':words[event['address']]=event['word']
            if step==2:
                arms.append(dict(channel=channel,requested_tick=event['tick'],desired_control=0x10001,
                    destination=words[base+0xc0],configuration_sha256=hashes[channel],
                    callback=(0x11f5e60,0x11f5e20)[channel],physical_irq=self.IRQ_CONTRACT[channel][1]))
            previous_tick=event['tick']
        if (marker['arms']!=arms or marker['state']!=
                ('armed' if len(marker['accesses'])==6 else 'applying' if marker['accesses'] else 'selected')):
            raise UnsupportedDevice('AF image rearm registration/arm lifetime changed')
        armed_words=deepcopy(words)
        refresh=marker.get('refresh');history=marker.get('refresh_history',[])
        if (type(history) is not list or len(history)>250 or history and refresh is None
                or any(type(r) is not dict or r.get('state')!='complete' for r in history)):
            raise UnsupportedDevice('AF image refresh flat historical bound/lifetime changed')
        previous_pair=0
        for refresh in history+([refresh] if refresh is not None else []):
            if (len(marker['accesses'])!=6 or type(refresh) is not dict
                    or type(refresh.get('accesses')) is not list
                    or not 1<=len(refresh['accesses'])<=2*len(self.CONFIG_ORDER)
                    or refresh.get('state')!=('complete' if len(refresh['accesses'])==2*len(self.CONFIG_ORDER) else 'applying')
                    or set(refresh)!=({'state','frame','input_sha256','parameters','accesses'}
                        | ({'completed_tick'} if refresh['state']=='complete' else set())
                        | ({'mode'} if 'mode' in refresh else set()))):
                raise UnsupportedDevice('AF image refresh plain schema or paired budget changed')
            clock=self._afs_refresh_clock(refresh)
            pair_id=clock['pair_id'];mode=refresh.get('mode',5)
            gap=pair_id!=previous_pair+1
            if (pair_id<=previous_pair or mode not in (5,6)
                    or clock['source0']['asserted_tick'] is None):
                raise UnsupportedDevice('AF image refresh precedes its first pixel source0')
            if mode==6 or gap:
                preview=marker.get('preview_data')
                if (preview is None or preview.get('state')!='acknowledged'
                        or any(type(e) is not dict or type(e.get('tick')) is not int
                               for e in preview.get('acknowledgments',[]))
                        or len(preview.get('acknowledgments',[]))!=2
                        or max(e['tick'] for e in preview['acknowledgments'])>refresh['accesses'][0]['tick']):
                    raise UnsupportedDevice('AF image refresh gap precedes its real paired preview ACK')
                if previous_pair==0:
                    raise UnsupportedDevice('AF image refresh gap cannot replace the first pixel geometry')
                self._afs_refresh_gap(previous_pair,pair_id)
            expected=self._afs_refresh_words(refresh['parameters'])
            for index,event in enumerate(refresh['accesses']):
                channel,step=divmod(index,len(self.CONFIG_ORDER));address=self.BASES[channel]+self.CONFIG_ORDER[step]
                if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                        or (event['operation'],event['address'],event['word'])!=('write',address,expected[index])
                        or type(event['word']) is not int or type(event['old_word']) is not int
                        or event['old_word']!=words[address] or type(event['cpu']) is not int or event['cpu']!=0
                        or type(event['step']) is not int or event['step']!=index
                        or type(event['tick']) is not int
                        or not max(previous_tick,clock['source0']['asserted_tick'])<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('AF image refresh changed its latched setter words/order/history')
                words[address]=event['word'];previous_tick=event['tick']
            if refresh['state']=='complete' and (type(refresh['completed_tick']) is not int or refresh['completed_tick']!=previous_tick):
                raise UnsupportedDevice('AF image refresh completion clock changed')
            previous_pair=pair_id
        if 'preview_data' in marker:
            data=self._validate_afs_preview_data(marker,armed_words)
            for channel,event in enumerate(data['acknowledgments']):
                if event is not None:words[self.BASES[channel]+0xd4]=1
        return self._validate_afs_stop(marker,words,previous_tick)

    def _afs_stop_marker(self):
        return self.banks[0].get('closed_reset',{}).get('af_rearm',{}).get('final_stop')

    def _afs_stop_waiting(self):
        marker=self._afs_stop_marker()
        return marker is not None and marker['state']=='waiting-ack'

    def _afs_stop_prefix(self,marker):
        closed=self.banks[0]['closed_reset']
        sources=[d for d in self.board.devices if type(d).__name__=='AfsFrameSource']
        if len(sources)>1 or any(d.board is not self.board for d in sources):
            raise UnsupportedDevice('Final AF image close has ambiguous terminal source ownership')
        # Isolated driver fixtures supply read-only source facades. A real
        # whole-machine owner additionally seals its terminal flat ledger.
        source=sources[0].request if len(sources)==1 else None
        if type(source) is dict and ('capture_initialization' in source or 'still_initialization' in source):
            close=marker.get('final_stop')
            if type(close) is not dict or close.get('state')!='completed':
                raise UnsupportedDevice('Capture clocks cannot replace an unfinished AF image close')
            # Later capture clocks have their own physical ACK/EOI ledgers.
            # Validate them independently, then retain the complete original
            # AF Source in this historical image-stop hash.
            from . import afs_capture_clock, afs_still_initialization_clock
            afs_capture_clock.validate(sources[0])
            if 'still_initialization' in source:
                afs_still_initialization_clock.validate(sources[0])
            source={k:v for k,v in source.items() if k not in ('capture_initialization','still_initialization')}
        return self._afs_digest(dict(legacy=self._closed_reset_history_hash(),
            closed={k:v for k,v in closed.items() if k!='af_rearm'},
            rearm={k:v for k,v in marker.items() if k!='final_stop'},source=source))

    def _validate_afs_stop_current(self):
        marker=self.banks[0]['closed_reset']['af_rearm'];close=marker['final_stop']
        # These current polls consume only the already admitted stop. Rehash
        # its complete old prefix rather than recursively replaying all
        # prior physical clocks and producer activations on every busy poll.
        words=self._validate_afs_stop(marker,dict(close['prior_words']),close['started_tick'])
        if words!=self.registers:
            raise UnsupportedDevice('Final AF image close changed its current register projection')

    def _afs_stop_route(self,channel,enabled):
        self._afs_rearm_route(channel,enabled)
        base=0xccc1f0+16*channel
        expected=struct.pack('<2I',(0x11f5e60,0x11f5e20)[channel],1) if enabled else bytes(8)
        if bytes(self.board.read_memory(base,8,cpu_id=0))!=expected:
            raise UnsupportedDevice('Final AF image close changed its live stock software IRQ client')

    def _afs_stop_admission(self,marker,core):
        self._idle_quiescent()
        read=lambda a,n=4:int.from_bytes(self.board.read_memory(a,n,cpu_id=0),'little')
        preview=marker.get('preview_data');mode=read(0x1625f5c)
        if (self.stop_profile!=IDLE_STOP_PROFILE or type(core) is not int or core!=0
                or marker['state']!='armed' or len(marker['accesses'])!=6
                or any(r.get('state')!='complete' for r in marker.get('refresh_history',[]))
                or marker.get('refresh',{}).get('state')!='complete'
                or mode!=14
                or preview is not None and preview['state']!='acknowledged'
                or read(0x16223f8,1)!=4 or read(0x15f8828,2)!=18
                or read(0xccde04)!=0 or read(0x15f63a4)!=3
                or read(0x15f64a4)!=4 or read(0x15f64a8)!=0x132682c
                or read(0x15f669c)!=0 or read(0x15f66a0)!=0x300
                or read(0x15f6e50)!=0 or read(0x15f68a4)!=0
                or any(self.registers[base]!=0x10001 for base in self.BASES)):
            raise UnsupportedDevice('Final AF preview close differs from the drained stock3F24/11F9810 caller')
        for channel in (0,1):self._afs_stop_route(channel,True)
        return dict(profile=AFS_PREVIEW_STOP_PROFILE,cpu=core,state='write-zero',
            started_tick=self.board.virtual_ticks,prior_words=dict(self.registers),
            prefix_sha256=self._afs_stop_prefix(marker),requests=[])

    def _validate_afs_stop(self,marker,words,previous_tick):
        close=marker.get('final_stop')
        if close is None:return words
        if (type(close) is not dict or set(close)!={'profile','cpu','state','started_tick','prior_words','prefix_sha256','requests'}
                or close['profile']!=AFS_PREVIEW_STOP_PROFILE or type(close['cpu']) is not int or close['cpu']!=0
                or self.stop_profile!=IDLE_STOP_PROFILE or type(self.stop_ticks) is not int or self.stop_ticks<1
                or close['prior_words']!=words or close['prefix_sha256']!=self._afs_stop_prefix(marker)
                or type(close['started_tick']) is not int or not previous_tick<=close['started_tick']<=self.board.virtual_ticks
                or type(close['requests']) is not list or not 1<=len(close['requests'])<=2):
            raise UnsupportedDevice('Final AF preview close changed its retained prefix/schema')
        previous_tick=close['started_tick']
        for channel,request in enumerate(close['requests']):
            events=request.get('accesses') if type(request) is dict else None
            suffix={'requested_tick','due_tick','acknowledged_tick','acknowledged_poll_count'} if events and len(events)>1 else set()
            if (type(request) is not dict or set(request)!=({'channel','state','accesses'}|suffix)
                    or type(request['channel']) is not int or request['channel']!=channel
                    or type(events) is not list or not 1<=len(events)<=2+self.CLOSED_RESET_POLL_LIMIT):
                raise UnsupportedDevice('Final AF image bank close lost its ordered request')
            base=self.BASES[channel];polls=[];state='write-zero';seen_one=False
            ack=request.get('acknowledged_tick');boundary=request.get('acknowledged_poll_count')
            if len(events)>1:
                due=request['due_tick'];requested=request['requested_tick']
                if (type(requested) is not int or requested!=events[1]['tick']
                        or type(due) is not int or due!=requested+self.stop_ticks or not 0<=due<1<<64
                        or ack is not None and (type(ack) is not int or not due<=ack<=self.board.virtual_ticks)
                        or (boundary is not None if ack is None else type(boundary) is not int or boundary<0)):
                    raise UnsupportedDevice('Final AF image close changed its declared stop delay')
                state='waiting-ack' if ack is None else 'acknowledged-awaiting-poll'
            for index,event in enumerate(events):
                if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                        or type(event['cpu']) is not int or event['cpu']!=0
                        or type(event['step']) is not int or event['step']!=index
                        or type(event['word']) is not int or type(event['tick']) is not int
                        or not previous_tick<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('Final AF image close changed its typed access chronology')
                if index==0:expected=('read',base,0x10001)
                elif index==1:expected=('write',base,0);words[base]=0
                else:
                    if seen_one:raise UnsupportedDevice('Final AF image close repeats an already observed stop poll')
                    after=ack is not None and len(polls)>=boundary
                    expected=('read',base,1 if after else 0)
                    if ack is not None and (event['tick']<ack if after else event['tick']>ack):
                        raise UnsupportedDevice('Final AF image close poll crosses its acknowledgment boundary')
                    polls.append(event['tick']);seen_one=after
                    if seen_one:state='poll-observed'
                if (event['operation'],event['address'],event['word'])!=expected:
                    raise UnsupportedDevice('Final AF image close differs from92BB28 read/disable/poll')
                previous_tick=event['tick']
            if ack is not None:
                if boundary>len(polls):raise UnsupportedDevice('Final AF image close lost its ACK poll boundary')
                words[base]=1
            if (request['state']!=state or channel+1<len(close['requests']) and state!='poll-observed'):
                raise UnsupportedDevice('Final AF image close changed its paired phase')
        state='completed' if len(close['requests'])==2 and state=='poll-observed' else state
        if close['state']!=state:raise UnsupportedDevice('Final AF paired image close changed its current phase')
        return words

    def _afs_stop_access(self,marker,operation,core,channel,offset,value):
        close=marker.get('final_stop')
        if close is None:
            if (operation,channel,offset)!=('read',0,0):return False
            close=self._afs_stop_admission(marker,core)
        if close['state']=='completed':
            raise UnsupportedDevice('Final AF preview owner has already stopped both image banks')
        beginning=not close['requests'] or close['state']=='poll-observed'
        if beginning:
            request=dict(channel=len(close['requests']),state='write-zero',accesses=[])
            expected=('read',request['channel'],0,0x10001)
        else:
            request=close['requests'][-1]
            expected=(('write',request['channel'],0,0) if request['state']=='write-zero'
                else ('read',request['channel'],0,0 if request['state']=='waiting-ack' else 1))
            if len(request['accesses'])-2>=self.CLOSED_RESET_POLL_LIMIT:
                raise UnsupportedDevice('Final AF image close exhausted its bounded polling')
        word=self.registers[self.BASES[channel]+offset] if operation=='read' else value
        if type(core) is not int or core!=0 or (operation,channel,offset,word)!=expected:
            raise UnsupportedDevice('Final AF image close requires its exact next bank disable/poll access')
        self._idle_quiescent();self._afs_stop_route(channel,True)
        if channel==1:self._afs_stop_route(0,False)
        tick=self.board.virtual_ticks
        previous=close['requests'][-1]['accesses'][-1]['tick'] if close['requests'] else close['started_tick']
        if type(tick) is not int or not previous<=tick<1<<64:
            raise UnsupportedDevice('Final AF image close clock changed')
        if operation=='write':
            if tick+self.stop_ticks>=1<<64:raise UnsupportedDevice('Final AF image stop delay overflows')
            request.update(requested_tick=tick,due_tick=tick+self.stop_ticks,acknowledged_tick=None,acknowledged_poll_count=None)
        event=dict(operation=operation,address=self.BASES[channel]+offset,word=word,cpu=core,tick=tick,step=len(request['accesses']))
        request['accesses'].append(event)
        request['state']='write-zero' if beginning else 'waiting-ack' if operation=='write' or word==0 else 'poll-observed'
        if beginning:close['requests'].append(request)
        close['state']='completed' if len(close['requests'])==2 and request['state']=='poll-observed' else request['state']
        marker['final_stop']=close
        if operation=='write':
            old=self.registers[event['address']];self.registers[event['address']]=word
            self.writes.append(dict(cpu=core,address=hex(event['address']),value=hex(word),old=hex(old),tick=tick))
        self.busy=self._afs_stop_waiting()
        self.board.record('Final AF image bank close retained',**event,
            limits='Drained92BB28 disable/poll only; old arms/configuration/data unchanged; no payload or IRQ')
        return True

    def _tick_afs_stop(self,now):
        close=self._afs_stop_marker()
        if close is None or close['state']!='waiting-ack':return
        request=close['requests'][-1]
        if now<request['due_tick']:return
        self._validate_closed_reset()
        self.registers[self.BASES[request['channel']]]=1
        request.update(acknowledged_tick=now,acknowledged_poll_count=len(request['accesses'])-2,state='acknowledged-awaiting-poll')
        close['state']=request['state']
        self.board.record('Final AF image stop bit0 acknowledged (unmeasured)',channel=request['channel'],tick=now,
            limits='Same declared quiescent bit0 stop delay; no new DMA/D4/IRQ/callback')

    @staticmethod
    def _afs_preview_resample(raw,scene,output,numpy=True):
        """Declared full-field nearest-neighbor YYCbCr; row padding is128."""
        from .measurement_scene import _numpy
        width,height,stride=scene['width'],scene['height'],scene['stride_bytes']
        ow,oh,os=output['width'],output['height'],output['stride_bytes']
        if (any(type(v) is not int or v<1 for v in (width,height,stride,ow,oh,os))
                or type(raw) is not bytes or len(raw)!=stride*height or width&1 or ow&1
                or stride<2*width or os<2*ow or os*oh>4*1024*1024):
            raise UnsupportedDevice('AF preview scene/resampling geometry is unsupported')
        if numpy and _numpy is not None:
            pairs=_numpy.frombuffer(raw,dtype=_numpy.uint8).reshape(height,stride)[:,:width*2].reshape(height,width//2,4)
            out=_numpy.full((oh,os),128,dtype=_numpy.uint8)
            dst=out[:,:ow*2].reshape(oh,ow//2,4)
            yy=_numpy.arange(oh)*height//oh;xx=_numpy.arange(ow)*width//ow
            luma=pairs[:,:,:2].reshape(height,width)
            dst[:,:,:2]=luma[yy[:,None],xx[None,:]].reshape(oh,ow//2,2)
            cx=(_numpy.arange(ow//2)*2+1)*width//ow//2
            dst[:,:,2:]=pairs[yy[:,None],cx[None,:],2:]
            return out.tobytes()
        out=bytearray([128])*(os*oh)
        for y in range(oh):
            source=y*height//oh*stride
            for x in range(0,ow,2):
                x0=x*width//ow;x1=(x+1)*width//ow;chroma=((x+1)*width//ow)//2*4
                begin=y*os+x*2
                out[begin:begin+4]=bytes((raw[source+(x0//2)*4+(x0&1)],
                    raw[source+(x1//2)*4+(x1&1)],raw[source+chroma+2],raw[source+chroma+3]))
        return bytes(out)

    def _afs_preview_marker(self):
        return self.banks[0].get('closed_reset',{}).get('af_rearm',{}).get('preview_data')

    def arm_afs_preview_data(self,profile=AFS_PREVIEW_DATA_PROFILE):
        """Select one future Source0-latched pair, preserving all old jobs."""
        self._validate_afs_data();self._validate_closed_reset();self._idle_quiescent()
        marker=self.banks[0].get('closed_reset',{}).get('af_rearm')
        clock=self._afs_refresh_clock();read=lambda a,n=4:int.from_bytes(self.board.read_memory(a,n,cpu_id=0),'little')
        now=self.board.virtual_ticks
        if (profile!=AFS_PREVIEW_DATA_PROFILE or marker is None or marker['state']!='armed'
                or 'preview_data' in marker or 'refresh' in marker or len(self.data_history)!=3
                or clock['source0']['state']!='waiting' or not marker['accesses'][-1]['tick']<=now<clock['source0']['due_tick']
                or read(0x16223f8,1)!=12 or read(0x1625f5c)!=5 or read(0x1625f4c)!=1
                or read(0x16260bc)!=0 or read(0x15f6e50)!=0
                or read(0x15f64a4)!=1 or read(0x15f64a8)!=0x132682c or read(0x15f669c)!=0 or read(0x15f66a0)!=0x300
                or type(self.data_ticks) is not int or not 1<=self.data_ticks<=400000
                or clock['source0']['due_tick']+self.data_ticks>=1<<64):
            raise UnsupportedDevice('AF preview data requires the future first pixel and its two complete desired arms')
        self._afs_routes()
        item=self.data_history[-1]['outputs'][0]
        scene={key:item[key] for key in ('address','bytes','width','height','stride_bytes','payload_sha256')}
        scene.update(epoch=deepcopy(self.data_history[-1]['epoch']))
        raw=bytes(self.board.read_memory(int(scene['address'],16),scene['bytes'],cpu_id=0))
        if hashlib.sha256(raw).hexdigest()!=scene['payload_sha256']:
            raise UnsupportedDevice('AF preview data scene differs from the acknowledged image input')
        outputs=[self._data_tuple(c) for c in (0,1)];scene_begin,scene_end=ram_span(int(scene['address'],16),scene['bytes'])
        ranges=[(scene_begin,scene_end)]
        for output in outputs:
            begin,end=output['physical_begin'],output['physical_end']
            if output['bytes']>4*1024*1024 or any(begin<high and low<end for low,high in ranges):
                raise UnsupportedDevice('AF preview output overlaps another output or its immutable scene')
            ranges.append((begin,end))
        latch=dict(frame=clock['frame'],input_sha256=clock['input_sha256'],armed_tick=now,
            source_due_tick=clock['source0']['due_tick'],
            data_ticks=self.data_ticks,scene=scene,outputs=outputs,
            old_history_sha256=self._afs_digest(self.data_history),words=dict(self.registers))
        data=dict(profile=profile,state='waiting-source0',latch=latch,latch_sha256=self._afs_digest(latch),
            epoch=None,due_tick=None,completed_tick=None,payload_sha256=None,acknowledgments=[None,None])
        marker['preview_data']=data
        self.board.record('First AF preview image pair admitted',profile=profile,frame=clock['frame'],
            limits='Pre-Source0 tuple latch and full-field nearest-neighbor supplied YYCbCr are declared; no input, payload or IRQ yet')
        return deepcopy(data)

    def _validate_afs_preview_data(self,marker=None,armed_words=None):
        data=marker.get('preview_data') if marker is not None else self._afs_preview_marker()
        if data is None:return None
        if (type(data) is not dict or set(data)!={'profile','state','latch','latch_sha256','epoch','due_tick',
                'completed_tick','payload_sha256','acknowledgments'} or data['profile']!=AFS_PREVIEW_DATA_PROFILE
                or data['state'] not in ('waiting-source0','pending','data-before-irq','acknowledged')
                or type(data['latch']) is not dict or data['latch_sha256']!=self._afs_digest(data['latch'])
                or type(data['acknowledgments']) is not list or len(data['acknowledgments'])!=2):
            raise UnsupportedDevice('AF preview data plain identity/state changed')
        latch=data['latch'];clock=self._afs_refresh_clock(latch)
        if (set(latch)!={'frame','input_sha256','armed_tick','source_due_tick','data_ticks','scene','outputs','old_history_sha256','words'}
                or type(latch['armed_tick']) is not int or not 0<=latch['armed_tick']<clock['source0']['due_tick']
                or clock['pair_id']!=1 or latch['source_due_tick']!=clock['source0']['due_tick']
                or latch['data_ticks']!=self.data_ticks or type(latch['data_ticks']) is not int or not 1<=latch['data_ticks']<=400000
                or latch['old_history_sha256']!=self._afs_digest(self.data_history)
                or type(latch['outputs']) is not list or len(latch['outputs'])!=2
                or type(latch['words']) is not dict or set(latch['words'])!=set(self.registers)
                or armed_words is not None and latch['words']!=armed_words):
            raise UnsupportedDevice('AF preview data immutable old history/latch changed')
        original=self.data_history[-1]
        scene={key:original['outputs'][0][key] for key in ('address','bytes','width','height','stride_bytes','payload_sha256')}
        scene['epoch']=deepcopy(original['epoch'])
        if len(self.data_history)!=3 or original['state']!='acknowledged' or latch['scene']!=scene:
            raise UnsupportedDevice('AF preview data lost its acknowledged supplied scene')
        view=latch['words'];ranges=[ram_span(int(scene['address'],16),scene['bytes'])]
        for c,base in enumerate(self.BASES):
            if view[base]!=0x10001 or any(not self._config_valid(o,view[base+o]) for o in self.ALL_CONFIG_MASKS):
                raise UnsupportedDevice('AF preview latch control/configuration domains changed')
            width,height=(view[base+0xb0]&0x7ff)+1,((view[base+0xb0]>>16)&0x7ff)+1
            stride=2*view[base+0xc4];begin,end=ram_span(view[base+0xc0],stride*height)
            expected=dict(channel=c,address=hex(view[base+0xc0]),physical_begin=begin,physical_end=end,
                width=width,height=height,stride_bytes=stride,bytes=stride*height,
                source_width=(view[base+4]&0xfff)+1,source_height=((view[base+4]>>16)&0xfff)+1,
                generation=self.banks[c]['active_generation'],route=0,
                configuration_sha256=hashlib.sha256(b''.join(struct.pack('<II',base+o,view[base+o])
                    for o in sorted(self.ALL_CONFIG_MASKS))).hexdigest(),
                assumptions='Candidate YYCbCr; stride=2*C4; full analytic source resampling; sampled tuple at arm/update')
            if (width&1 or stride<2*width or stride*height>4*1024*1024
                    or latch['outputs'][c]!=expected or any(begin<high and low<end for low,high in ranges)):
                raise UnsupportedDevice('AF preview data changed its bounded pre-source output tuple/span')
            ranges.append((begin,end))
        edge=clock['source0'];epoch=None if edge['asserted_tick'] is None else {key:edge[key] for key in
            ('profile','generation','ordinal','attempt','frame','cpu','source','physical_irq','asserted_tick')}
        if data['state']=='waiting-source0':
            if any(data[key] is not None for key in ('epoch','due_tick','completed_tick','payload_sha256')) or any(data['acknowledgments']):
                raise UnsupportedDevice('AF preview waiting pair already owns physical completion')
            return data
        if (epoch is None or data['epoch']!=epoch or type(data['due_tick']) is not int
                or data['due_tick']!=epoch['asserted_tick']+latch['data_ticks']):
            raise UnsupportedDevice('AF preview data lost its real Source0 epoch/delay')
        if data['state']=='pending':
            if data['completed_tick'] is not None or data['payload_sha256'] is not None or any(data['acknowledgments']):
                raise UnsupportedDevice('AF preview pending pair has premature data/ACK')
            return data
        if (type(data['completed_tick']) is not int or not data['due_tick']<=data['completed_tick']<=self.board.virtual_ticks
                or type(data['payload_sha256']) is not list or len(data['payload_sha256'])!=2
                or any(type(s) is not str or len(s)!=64 for s in data['payload_sha256'])):
            raise UnsupportedDevice('AF preview payload completion changed')
        for channel,event in enumerate(data['acknowledgments']):
            if event is not None and (type(event) is not dict or set(event)!={'cpu','address','word','tick'}
                    or (event['cpu'],event['address'],event['word'])!=(0,self.BASES[channel]+0xd4,1)
                    or type(event['tick']) is not int or not data['completed_tick']<=event['tick']<=self.board.virtual_ticks):
                raise UnsupportedDevice('AF preview changed its actual paired D4 acknowledgment')
        if (data['state']=='acknowledged')!=all(e is not None for e in data['acknowledgments']):
            raise UnsupportedDevice('AF preview paired ACK state changed')
        return data

    def _afs_preview_ack(self,operation,core,channel,offset,value):
        data=self._afs_preview_marker()
        if data is None or offset!=0xd4:return False
        self._validate_afs_preview_data();line=self.IRQ_CONTRACT[channel][1]
        if (operation!='write' or type(core) is not int or core!=0 or value!=1
                or data['state']!='data-before-irq' or data['acknowledgments'][channel] is not None
                or self.board.interrupts.banks[0].active!=line):
            raise UnsupportedDevice('AF preview D4 ACK requires its real delivered bank IRQ')
        self._afs_routes(pending=True)
        address=self.BASES[channel]+offset;old=self.registers[address];tick=self.board.virtual_ticks
        data['acknowledgments'][channel]=dict(cpu=core,address=address,word=1,tick=tick)
        if all(e is not None for e in data['acknowledgments']):data['state']='acknowledged'
        self.registers[address]=1;self.writes.append(dict(cpu=core,address=hex(address),value='0x1',old=hex(old),tick=tick))
        self.board.record('AF preview actual D4 acknowledgment retained',channel=channel,tick=tick)
        return True

    def _tick_afs_preview_data(self,now):
        data=self._afs_preview_marker()
        if data is None or data['state'] in ('data-before-irq','acknowledged'):return
        if data['state']=='waiting-source0' and now<data['latch']['source_due_tick']:return
        if data['state']=='pending' and now<data['due_tick']:return
        self._validate_afs_preview_data();latch=data['latch'];clock=self._afs_refresh_clock(latch)
        edge=clock['source0']
        if edge['asserted_tick'] is None:return
        if data['state']=='waiting-source0':
            data.update(state='pending',epoch={key:edge[key] for key in
                ('profile','generation','ordinal','attempt','frame','cpu','source','physical_irq','asserted_tick')},
                due_tick=edge['asserted_tick']+latch['data_ticks'])
        if now<data['due_tick']:return
        self._afs_routes();scene=latch['scene']
        raw=bytes(self.board.read_memory(int(scene['address'],16),scene['bytes'],cpu_id=0))
        if hashlib.sha256(raw).hexdigest()!=scene['payload_sha256']:
            raise UnsupportedDevice('AF preview immutable scene changed before DMA')
        payloads=[self._afs_preview_resample(raw,scene,item) for item in latch['outputs']]
        for item,payload in zip(latch['outputs'],payloads):
            self.board.write_memory(int(item['address'],16),payload)
        data.update(state='data-before-irq',completed_tick=now,
                    payload_sha256=[hashlib.sha256(p).hexdigest() for p in payloads])
        for item in latch['outputs']:self.board.queue_irq(item['route'],self.IRQ_CONTRACT[item['channel']][1])
        self.board.record('AF preview payload pair delivered before IRQ',frame=latch['frame'],
            outputs=latch['outputs'],payload_sha256=data['payload_sha256'],
            limits='One source-latched declared supplied-scene pair; stock D4/client/mailbox completion remains outstanding')

    def _afs_refresh_clock(self,marker=None,pair_id=1):
        from .afs_frame_source import af_requests
        matches=[r for r in af_requests(self.board) if r.get('kind')=='pixel'
                 and (r.get('pair_id')==pair_id if marker is None else
                      (r.get('frame'),r.get('input_sha256'))==(marker['frame'],marker['input_sha256']))]
        n=matches[0].get('pair_id') if len(matches)==1 else None
        if (len(matches)!=1 or matches[0].get('canceled_tick') is not None
                or type(n) is not int or not 1<=n<=251
                or matches[0].get('phase_at_arm')!=(12 if n==1 else 13)
                or matches[0].get('source_count_at_arm')!=n-1 or matches[0].get('data_count_at_arm')!=n-1):
            raise UnsupportedDevice('AF image refresh requires its unique contiguous contrast pixel source')
        return matches[0]

    def _afs_refresh_words(self,parameters):
        # Exact packing of the seven unchanged92A9FC..92B7D0 setters, bound
        # to the real1205E9C caller block rather than guessed frame geometry.
        if type(parameters) is not str or len(parameters)!=256:
            raise UnsupportedDevice('AF image refresh lacks its retained128-byte caller block')
        try:data=bytes.fromhex(parameters)
        except ValueError as error:raise UnsupportedDevice('AF image refresh caller block is malformed') from error
        if len(data)!=128 or data.hex()!=parameters:
            raise UnsupportedDevice('AF image refresh caller block is not canonical128-byte hex')
        result=[]
        for start in (8,0x40):
            u16=lambda offset:struct.unpack_from('<H',data,start+offset)[0]
            u32=lambda offset:struct.unpack_from('<I',data,start+offset)[0]
            result.extend(((u16(0)-1)&0xfff|((u16(2)-1)&0xfff)<<16,
                (u32(4)==1)<<31|u16(8),u16(10),u32(12)|u32(16)<<16,
                ((u16(20)-1)&0x7ff)|((u16(22)-1)&0x7ff)<<16,
                (u16(24)&0x7ff)<<16|((u16(26)-1)&0x7ff),
                (u32(28)==1)<<24|(u16(32)&0xff)<<16|u32(36)<<4|u32(40),
                u32(44),u16(48),data[start+52]))
        if any(not self._config_valid(self.CONFIG_ORDER[index%len(self.CONFIG_ORDER)],word)
               for index,word in enumerate(result)):
            raise UnsupportedDevice('AF image refresh caller fields exceed the stock setter domains')
        return result

    def _afs_refresh_gap(self,previous_pair,pair_id):
        # IMGDRV5/6 need not dirty geometry on every pixel after preview ACK.
        # Retain only setters that firmware actually executes; skipped pairs remain in
        # their independently validated physical/pose history.
        from .afs_frame_source import af_requests
        records=af_requests(self.board) if pair_id>previous_pair+1 else []
        for n in range(previous_pair+1,pair_id):
            matches=[r for r in records if r.get('kind')=='pixel' and r.get('pair_id')==n]
            if (len(matches)!=1 or matches[0].get('consumed_tick') is None
                    or any(matches[0][key].get('state')!='acknowledged'
                           or matches[0][key].get('eoi_tick') is None for key in ('source0','source1'))):
                raise UnsupportedDevice('AF geometry gap lacks its closed intervening physical pixel pairs')

    def _afs_refresh_access(self,marker,operation,core,channel,offset,value):
        refresh=marker.get('refresh')
        if refresh is None and (operation,channel,offset)!=('write',0,self.CONFIG_ORDER[0]):return False
        index=len(refresh['accesses']) if refresh is not None else 0
        following=index==2*len(self.CONFIG_ORDER)
        if following and (operation,channel,offset)!=('write',0,self.CONFIG_ORDER[0]):return False
        read=lambda a,n=4:int.from_bytes(self.board.read_memory(a,n,cpu_id=0),'little')
        pair_id=read(0x1625f4c)-1 if following else self._afs_refresh_clock(refresh)['pair_id'] if refresh is not None else 1
        clock=self._afs_refresh_clock(None if following else refresh,pair_id)
        if following:index=0
        preview=self._afs_preview_marker();mode=read(0x1625f5c)
        previous_pair=self._afs_refresh_clock(refresh)['pair_id'] if following else (
            self._afs_refresh_clock(marker['refresh_history'][-1])['pair_id'] if marker.get('refresh_history') else 0)
        gap=pair_id!=previous_pair+1
        continuation=((mode==6 or mode==5 and gap) and pair_id>1 and previous_pair>0
                      and preview is not None and preview['state']=='acknowledged')
        if pair_id<=previous_pair or mode==5 and gap and not continuation:
            raise UnsupportedDevice('AF image geometry source order differs from its actual caller count')
        if continuation:self._afs_refresh_gap(previous_pair,pair_id)
        phase=read(0x16223f8,1);consumed=clock.get('consumed_tick')
        edges=[clock.get(key) for key in ('source0','source1')]
        # Source0 queues401C; Source1's real3F00 consumer may reach12FC110
        # and AF13 before that queued1205E9C setter runs. Admit this first
        # pose ordering only after both physical EOIs and the pose-array write.
        first_pose_consumed=(pair_id==1 and phase==13
            and type(consumed) is int and consumed<=self.board.virtual_ticks
            and all(type(edge) is dict and edge.get('state')=='acknowledged'
                and type(edge.get('eoi_tick')) is int and edge['eoi_tick']<=consumed for edge in edges)
            and type(clock.get('sampled_focus')) is int
            and read(0x16223fc,2)==1 and read(0x16223fe,2)==0
            and int.from_bytes(self.board.read_memory(0x162254c,2,cpu_id=0),'little',signed=True)==clock['sampled_focus'])
        if (type(core) is not int or core!=0 or clock['source0']['state'] not in ('asserted','acknowledged')
                or phase!=(12 if pair_id==1 else 13) and not first_pose_consumed
                or mode!=5 and not continuation or read(0x1625f4c)!=pair_id+1
                or read(0x16260bc)!=0 or read(0x15f6e50)!=0 or read(0x160e4e4)!=0x1800
                or read(0x15f63a4)!=3 or read(0x15f64a4)!=1 or read(0x15f64a8)!=0x132682c
                or read(0x15f64ac)!=0 or read(0x15f669c)!=0 or read(0x15f66a0)!=0x300
                or read(0x15f6e64)!=1):
            raise UnsupportedDevice('AF image refresh differs from the drained contiguous-pixel IMGDRV5/6 caller')
        if self._afs_preview_marker() is not None:self._afs_routes(pending=not continuation)
        else:
            for bank in (0,1):self._afs_rearm_route(bank,True)
        parameters=bytes(self.board.read_memory(0x160e504,0x80,cpu_id=0)).hex()
        if refresh is not None and not following and parameters!=refresh['parameters']:
            raise UnsupportedDevice('AF image refresh changed its active caller parameter block')
        expected=self._afs_refresh_words(parameters);expected_channel,step=divmod(index,len(self.CONFIG_ORDER))
        if (operation,channel,offset,value)!=('write',expected_channel,self.CONFIG_ORDER[step],expected[index]):
            raise UnsupportedDevice('AF image refresh requires its exact next paired caller-derived setter')
        tick=self.board.virtual_ticks;previous=(refresh['accesses'][-1]['tick'] if refresh is not None else marker['accesses'][-1]['tick'])
        if type(tick) is not int or not max(previous,clock['source0']['asserted_tick'])<=tick<1<<64:
            raise UnsupportedDevice('AF image refresh clock changed')
        if following:
            marker.setdefault('refresh_history',[]).append(deepcopy(refresh));refresh=None
        if refresh is None:
            refresh=dict(state='applying',frame=clock['frame'],input_sha256=clock['input_sha256'],
                         parameters=parameters,accesses=[]);marker['refresh']=refresh
            if mode==6:refresh['mode']=6
        if refresh.get('mode',5)!=mode:
            raise UnsupportedDevice('AF image refresh changed its current IMGDRV mode')
        address=self.BASES[channel]+offset;old=self.registers[address]
        event=dict(operation='write',address=address,word=value,old_word=old,cpu=core,tick=tick,step=index)
        refresh['accesses'].append(event);self.registers[address]=value
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=tick))
        if index==2*len(self.CONFIG_ORDER)-1:refresh.update(state='complete',completed_tick=tick)
        self.board.record('AF image-bank desired geometry refreshed',**event,
            pair_id=pair_id,limits='Contiguous pixel paired20 stores; all prior arms/data/geometry retained in flat history; no new data or IRQ')
        return True

    def _afs_rearm_access(self,record,operation,core,channel,offset,value):
        marker=record.get('af_rearm')
        if marker is None:return False
        index=len(marker['accesses'])
        if index==6:
            if self._afs_stop_access(marker,operation,core,channel,offset,value):return True
            return self._afs_refresh_access(marker,operation,core,channel,offset,value)
        expected_channel,step=divmod(index,3)
        expected=(('write',0xd8,1),('read',0,1),('write',0,0x10001))[step]
        address=self.BASES[channel]+offset;word=self.registers[address] if operation=='read' else value
        if (type(core) is not int or core!=0 or channel!=expected_channel
                or (operation,offset,word)!=expected):
            raise UnsupportedDevice('AF image rearm requires its exact next bank registration/arm access')
        clock=self._afs_rearm_clock(marker)
        read=lambda a,n=4:int.from_bytes(self.board.read_memory(a,n,cpu_id=0),'little')
        if (clock['source0']['state'] not in ('asserted','acknowledged')
                or read(0x16223f8,1)!=11 or read(0x1625f5c)!=5 or read(0x1625f4c)!=1
                or read(0x16260bc)!=0 or read(0x15f6e50)!=0):
            raise UnsupportedDevice('AF image rearm access precedes its actual first source0 callback')
        self._afs_rearm_route(channel,step!=0)
        if step!=0 and bytes(self.board.read_memory(0xccc1f0+16*channel,8,cpu_id=0))!=struct.pack('<2I',(0x11f5e60,0x11f5e20)[channel],1):
            raise UnsupportedDevice('AF image rearm software registration did not retain its stock callback')
        tick=self.board.virtual_ticks
        if type(tick) is not int or not (marker['accesses'][-1]['tick'] if index else marker['selected_tick'])<=tick<1<<64:
            raise UnsupportedDevice('AF image rearm clock changed')
        event=dict(operation=operation,address=address,word=word,cpu=core,tick=tick,step=index)
        marker['accesses'].append(event);marker['state']='armed' if index==5 else 'applying'
        if operation=='write':
            old=self.registers[address];self.registers[address]=word
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old),tick=tick))
        if step==2:
            marker['arms'].append(dict(channel=channel,requested_tick=tick,desired_control=word,
                destination=self.registers[self.BASES[channel]+0xc0],configuration_sha256=marker['configuration_sha256'][channel],
                callback=(0x11f5e60,0x11f5e20)[channel],physical_irq=self.IRQ_CONTRACT[channel][1]))
        self.board.record('AF image registration/desired arm retained',**event,
            limits='New plain arm ownership; old banks/data/registrations unchanged; no payload or IRQ')
        return True

    def arm_still_image_reset(self, profile):
        from . import image_still_reset
        return image_still_reset.arm(self,profile)

    def still_image_reset_summary(self):
        from . import image_still_reset
        return image_still_reset.summary(self)

    def arm_still_image_configuration(self, profile):
        from . import image_still_configuration
        return image_still_configuration.arm(self,profile)

    def still_image_configuration_summary(self):
        from . import image_still_configuration
        return image_still_configuration.summary(self)

    def arm_still_image_prepare(self, profile):
        from . import image_still_prepare
        return image_still_prepare.arm(self,profile)

    def still_image_prepare_summary(self):
        from . import image_still_prepare
        return image_still_prepare.summary(self)

    def arm_still_image_activation(self, profile):
        from . import image_still_activation
        return image_still_activation.arm(self,profile)

    def still_image_activation_summary(self):
        from . import image_still_activation
        return image_still_activation.summary(self)

    def arm_still_image_output(self, profile, *, scene, delay_ticks=1024):
        from . import afs_still_image_delivery
        return afs_still_image_delivery.arm(self,profile,scene=scene,delay_ticks=delay_ticks)

    def still_image_output_summary(self):
        from . import afs_still_image_delivery
        return afs_still_image_delivery.summary(self)

    def still_image_activation_anchor(self):
        from . import afs_still_image_delivery
        return afs_still_image_delivery.activation_anchor(self)

    def _validate_closed_reset(self):
        if getattr(self,'still_reset',None) is not None:
            from . import image_still_reset
            image_still_reset.validate(self)
            return
        record=self.banks[0].get('closed_reset')
        if record is None:return
        if (type(record) is not dict or type(record.get('cpu')) is not int
                or record['cpu'] not in (0,1) or type(record.get('requests')) is not list
                or not 1<=len(record['requests'])<=2 or type(record.get('prior_words')) is not dict
                or set(record)!=({'cpu','state','prior_words','requests','history_sha256'}
                    | ({'passive_reset'} if 'passive_reset' in record else set())
                    | ({'mode_configuration'} if 'mode_configuration' in record else set())
                    | ({'passive_configuration'} if 'passive_configuration' in record else set())
                    | ({'af_rearm'} if 'af_rearm' in record else set()))):
            raise UnsupportedDevice('Closed image reset ledger has unknown ownership/schema')
        self._closed_reset_ready(record['cpu'])
        if record.get('history_sha256')!=self._closed_reset_history_hash():
            raise UnsupportedDevice('Closed image reset changed its historical anchor')
        words=dict(record['prior_words'])
        prior_d4=1 if self.data_history else self.STOP_ACK
        if (set(words)!=set(self.registers) or any(type(w) is not int for w in words.values())
                or any(words[base]!=1 or words[base+0xd4]!=prior_d4 for base in self.BASES)):
            raise UnsupportedDevice('Closed image reset initial register projection changed')
        for channel,request in enumerate(record['requests']):
            events=request.get('accesses') if type(request) is dict else None
            if (type(events) is not list or not 1<=len(events)<=4+self.CLOSED_RESET_POLL_LIMIT
                    or request.get('channel')!=channel or type(request.get('channel')) is not int):
                raise UnsupportedDevice('Closed image reset request order/bound changed')
            base=self.BASES[channel];polls=[];state='write-zero';seen_one=False
            ack=request.get('acknowledged_tick');boundary=request.get('acknowledged_poll_count')
            if len(events)>1:
                requested=request.get('requested_tick');due=request.get('due_tick')
                if (type(self.stop_ticks) is not int or self.stop_ticks<1
                        or type(requested) is not int or requested!=events[1].get('tick')
                        or type(due) is not int or due!=requested+self.stop_ticks or not 0<=due<1<<64
                        or ack is not None and (type(ack) is not int or not due<=ack<=self.board.virtual_ticks)
                        or (boundary is not None if ack is None else type(boundary) is not int or boundary<0)):
                    raise UnsupportedDevice('Closed image reset delay/acknowledgment changed')
                state='waiting-ack' if ack is None else 'acknowledged-awaiting-poll'
            previous_tick=-1
            for index,event in enumerate(events):
                if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                        or type(event['word']) is not int or type(event['cpu']) is not int or event['cpu']!=record['cpu']
                        or type(event['step']) is not int or event['step']!=index
                        or type(event['tick']) is not int or not previous_tick<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('Closed image reset ordered access changed')
                actual=(event['operation'],event['address'],event['word'])
                if index==0:expected=('read',base,1)
                elif index==1:expected=('write',base,0);words[base]=0
                elif seen_one:
                    expected=('write',base+0xd4,self.STOP_ACK);state='completed'
                    if index!=len(events)-1:raise UnsupportedDevice('Closed image reset repeats after D4 acknowledgment')
                else:
                    after=ack is not None and len(polls)>=boundary
                    expected=('read',base,1 if after else 0)
                    if ack is not None and (event['tick']<ack if after else event['tick']>ack):
                        raise UnsupportedDevice('Closed image reset poll is on the wrong side of acknowledgment')
                    polls.append(event['tick']);seen_one=after
                    if seen_one:state='poll-observed'
                if actual!=expected:raise UnsupportedDevice('Closed image reset differs from paired stop/poll/D4 transaction')
                if event['operation']=='write':words[event['address']]=event['word']
                previous_tick=event['tick']
            if ack is not None:
                if boundary>len(polls):raise UnsupportedDevice('Closed image reset acknowledgment poll boundary changed')
                words[base]=1
            if (request.get('state')!=state or channel+1<len(record['requests']) and state!='completed'):
                raise UnsupportedDevice('Closed image reset request phase changed')
        previous_tick=record['requests'][-1]['accesses'][-1]['tick']
        configuration=record.get('mode_configuration')
        if configuration is not None:
            if (len(record['requests'])!=2 or record['state']!='completed'
                    or type(configuration) is not dict or type(configuration.get('accesses')) is not list
                    or not 1<=len(configuration['accesses'])<=2*len(self.CONFIG_ORDER)
                    or configuration.get('state')!=('complete' if len(configuration['accesses'])==2*len(self.CONFIG_ORDER) else 'applying')
                    or set(configuration)!=({'accesses','state','completed_tick'} if configuration['state']=='complete' else {'accesses','state'})):
                raise UnsupportedDevice('Closed image mode configuration schema/budget changed')
            for index,event in enumerate(configuration['accesses']):
                channel,step=divmod(index,len(self.CONFIG_ORDER));offset=self.CONFIG_ORDER[step]
                address=self.BASES[channel]+offset
                if (type(event) is not dict or set(event)!={'operation','address','word','old_word','cpu','tick','step'}
                        or event['operation']!='write' or event['address']!=address
                        or type(event['cpu']) is not int or event['cpu']!=record['cpu']
                        or type(event['step']) is not int or event['step']!=index
                        or not self._config_valid(offset,event['word'])
                        or type(event['old_word']) is not int or event['old_word']!=words[address]
                        or type(event['tick']) is not int or not previous_tick<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('Closed image mode configuration differs from its paired 10CBB44 setters')
                words[address]=event['word'];previous_tick=event['tick']
            if configuration['state']=='complete' and (type(configuration['completed_tick']) is not int or configuration['completed_tick']!=previous_tick):
                raise UnsupportedDevice('Closed image mode configuration completion clock changed')
        passive=record.get('passive_configuration')
        if passive is not None:
            if (configuration is None or configuration['state']!='complete'
                    or type(passive) is not dict or type(passive.get('accesses')) is not list
                    or not 1<=len(passive['accesses'])<=2*len(self.PASSIVE_CONFIG_ORDER)
                    or passive.get('state')!=('complete' if len(passive['accesses'])==2*len(self.PASSIVE_CONFIG_ORDER) else 'applying')
                    or set(passive)!=({'accesses','state','completed_tick'} if passive['state']=='complete' else {'accesses','state'})):
                raise UnsupportedDevice('Closed image passive configuration schema/budget changed')
            for index,event in enumerate(passive['accesses']):
                channel,step=divmod(index,len(self.PASSIVE_CONFIG_ORDER));offset=self.PASSIVE_CONFIG_ORDER[step]
                address=self.BASES[channel]+offset
                if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                        or event['operation']!='write' or event['address']!=address
                        or type(event['cpu']) is not int or event['cpu']!=record['cpu']
                        or type(event['step']) is not int or event['step']!=index
                        or not self._config_valid(offset,event['word'])
                        or type(event['tick']) is not int or not previous_tick<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('Closed image passive configuration differs from paired 10CB618 setters')
                words[address]=event['word'];previous_tick=event['tick']
            if passive['state']=='complete' and (type(passive['completed_tick']) is not int or passive['completed_tick']!=previous_tick):
                raise UnsupportedDevice('Closed image passive configuration completion clock changed')
        suffix=record.get('passive_reset')
        if suffix is not None:
            if (len(record['requests'])!=2 or record['state']!='completed'
                    or configuration is not None and configuration['state']!='complete'
                    or passive is not None and passive['state']!='complete'):
                raise UnsupportedDevice('Closed image passive reset precedes its paired D4 acknowledgments')
            if (type(suffix) is not dict or type(suffix.get('accesses')) is not list
                    or not 1<=len(suffix['accesses'])<=2*len(self.IDLE_RESET_SEQUENCE)
                    or suffix.get('state')!=('complete' if len(suffix['accesses'])==2*len(self.IDLE_RESET_SEQUENCE) else 'applying')
                    or set(suffix)!=({'accesses','state','completed_tick'} if suffix['state']=='complete' else {'accesses','state'})):
                raise UnsupportedDevice('Closed image passive reset schema/budget changed')
            for index,event in enumerate(suffix['accesses']):
                channel,step=divmod(index,len(self.IDLE_RESET_SEQUENCE))
                operation,offset=self.IDLE_RESET_SEQUENCE[step];address=self.BASES[channel]+offset
                expected=(0x10001000 if offset==0x10 else 0 if operation=='write' and offset==0x1c
                          else 0x10000 if offset>=0x30 else words[address])
                if (type(event) is not dict or set(event)!={'operation','address','word','cpu','tick','step'}
                        or event['operation']!=operation or event['address']!=address
                        or type(event['word']) is not int or event['word']!=expected
                        or type(event['cpu']) is not int or event['cpu']!=record['cpu']
                        or type(event['step']) is not int or event['step']!=index
                        or type(event['tick']) is not int or not previous_tick<=event['tick']<=self.board.virtual_ticks):
                    raise UnsupportedDevice('Closed image passive reset differs from its paired 10CA754 recipe')
                if operation=='write':words[address]=expected
                previous_tick=event['tick']
            if suffix['state']=='complete' and (type(suffix['completed_tick']) is not int or suffix['completed_tick']!=previous_tick):
                raise UnsupportedDevice('Closed image passive reset completion clock changed')
        if 'af_rearm' in record:
            if suffix is None or suffix['state']!='complete':
                raise UnsupportedDevice('AF image rearm precedes its complete paired reset recipe')
            words=self._validate_afs_rearm(record['af_rearm'],words,previous_tick)
        if (record.get('state')!=record['requests'][-1]['state'] or words!=self.registers):
            raise UnsupportedDevice('Closed image reset current register projection changed')

    def _closed_reset_configuration_access(self,record,operation,core,channel,offset,value):
        configuration=record.get('mode_configuration')
        index=len(configuration['accesses']) if configuration is not None else 0
        if (type(core) is not int or core!=record['cpu'] or record.get('passive_reset') is not None
                or index>=2*len(self.CONFIG_ORDER)):
            raise UnsupportedDevice('Closed image mode configuration owner or paired budget changed')
        expected_channel,step=divmod(index,len(self.CONFIG_ORDER))
        if ((operation,channel,offset)!=('write',expected_channel,self.CONFIG_ORDER[step])
                or not self._config_valid(offset,value)):
            raise UnsupportedDevice('Closed image mode configuration requires its exact next paired setter')
        tick=self.board.virtual_ticks
        previous=(configuration['accesses'][-1]['tick'] if configuration is not None else record['requests'][-1]['accesses'][-1]['tick'])
        if type(tick) is not int or not previous<=tick<1<<64:
            raise UnsupportedDevice('Closed image mode configuration clock changed')
        address=self.BASES[channel]+offset;old=self.registers[address]
        event=dict(operation='write',address=address,word=value,old_word=old,cpu=core,tick=tick,step=index)
        if configuration is None:
            configuration=dict(accesses=[],state='applying');record['mode_configuration']=configuration
        configuration['accesses'].append(event);self.registers[address]=value
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=tick))
        if len(configuration['accesses'])==2*len(self.CONFIG_ORDER):
            configuration.update(state='complete',completed_tick=tick)
        self.board.record('Closed image-bank mode setter retained',**event,
            limits='One paired desired configuration after quiet reset; prior arm/data/registration histories unchanged; no arm, image or IRQ')
        return True

    def _closed_reset_mode_passive_access(self,record,operation,core,channel,offset,value):
        configuration=record.get('mode_configuration')
        if configuration is None or configuration['state']!='complete':return False
        passive=record.get('passive_configuration');reset=record.get('passive_reset')
        # Both stock classes begin with W10=10001000. The next physical
        # access distinguishes 10CB618's W18 from 10CA754's R1C. Retain the
        # same first access when selecting the configuration continuation.
        shared=(passive is None and reset is not None and len(reset['accesses'])==1
                and (operation,channel,offset)==('write',0,0x18))
        first=(passive is None and reset is None and (operation,channel,offset)==('write',0,0x10)
               and value!=0x10001000)
        if not shared and not first and (passive is None or passive['state']=='complete'):return False
        index=len(reset['accesses']) if shared else len(passive['accesses']) if passive is not None else 0
        expected_channel,step=divmod(index,len(self.PASSIVE_CONFIG_ORDER))
        if (type(core) is not int or core!=record['cpu']
                or (operation,channel,offset)!=('write',expected_channel,self.PASSIVE_CONFIG_ORDER[step])
                or not self._config_valid(offset,value)):
            raise UnsupportedDevice('Closed image passive configuration requires its exact next paired setter')
        tick=self.board.virtual_ticks
        previous=(reset['accesses'][-1]['tick'] if shared else passive['accesses'][-1]['tick']
                  if passive is not None else configuration['completed_tick'])
        if type(tick) is not int or not previous<=tick<1<<64:
            raise UnsupportedDevice('Closed image passive configuration clock changed')
        if passive is None:
            passive=dict(accesses=list(reset['accesses']) if shared else [],state='applying')
            record['passive_configuration']=passive
            if shared:del record['passive_reset']
        address=self.BASES[channel]+offset;old=self.registers[address]
        event=dict(operation='write',address=address,word=value,cpu=core,tick=tick,step=index)
        passive['accesses'].append(event);self.registers[address]=value
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=tick))
        if len(passive['accesses'])==2*len(self.PASSIVE_CONFIG_ORDER):
            passive.update(state='complete',completed_tick=tick)
        self.board.record('Closed image-bank passive mode setter retained',**event,
            limits='One paired 10CB618 passive configuration before the reset recipe; prior arm/data/registration histories unchanged; no image or IRQ')
        return True

    def _closed_reset_passive_access(self,record,operation,core,channel,offset,value):
        suffix=record.get('passive_reset')
        index=len(suffix['accesses']) if suffix is not None else 0
        if type(core) is not int or core!=record['cpu'] or index>=2*len(self.IDLE_RESET_SEQUENCE):
            raise UnsupportedDevice('Closed image passive reset owner or paired budget changed')
        expected_channel,step=divmod(index,len(self.IDLE_RESET_SEQUENCE))
        expected_operation,expected_offset=self.IDLE_RESET_SEQUENCE[step]
        address=self.BASES[channel]+offset
        expected=(0x10001000 if offset==0x10 else 0 if operation=='write' and offset==0x1c
                  else 0x10000 if offset>=0x30 else self.registers[address])
        word=self.registers[address] if operation=='read' else value
        if (operation,channel,offset)!=(expected_operation,expected_channel,expected_offset) or type(word) is not int or word!=expected:
            raise UnsupportedDevice('Closed image passive reset requires its exact next paired access')
        tick=self.board.virtual_ticks
        previous=(suffix['accesses'][-1]['tick'] if suffix is not None else
                  record.get('passive_configuration',{}).get('completed_tick',
                      record.get('mode_configuration',{}).get('completed_tick',record['requests'][-1]['accesses'][-1]['tick'])))
        if type(tick) is not int or not previous<=tick<1<<64:
            raise UnsupportedDevice('Closed image passive reset clock changed')
        event=dict(operation=operation,address=address,word=word,cpu=core,tick=tick,step=index)
        if operation=='write':
            old=self.registers[address];self.registers[address]=word
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old),tick=tick))
        if suffix is None:suffix=dict(accesses=[],state='applying');record['passive_reset']=suffix
        suffix['accesses'].append(event)
        if len(suffix['accesses'])==2*len(self.IDLE_RESET_SEQUENCE):
            suffix.update(state='complete',completed_tick=tick)
        self.board.record('Closed image-bank passive reset access retained',**event,
            limits='One paired passive recipe after the closed stop; prior bank/data/arm/registration histories unchanged; no image or IRQ')
        return True

    def _closed_reset_access(self,operation,core,channel,offset,value=None):
        if getattr(self,'still_reset',None) is not None:
            from . import image_still_reset
            from . import image_still_configuration
            from . import afs_still_image_delivery
            if afs_still_image_delivery.owner(self) is not None:
                return afs_still_image_delivery.access(self,operation,core,channel,offset,value)
            from . import image_still_activation
            if image_still_activation.owner(self) is not None:
                return image_still_activation.access(self,operation,core,channel,offset,value)
            from . import image_still_prepare
            if image_still_prepare.owner(self) is not None:
                return image_still_prepare.access(self,operation,core,channel,offset,value)
            if image_still_configuration.owner(self) is not None:
                return image_still_configuration.access(self,operation,core,channel,offset,value)
            return image_still_reset.access(self,operation,core,channel,offset,value)
        record=self.banks[0].get('closed_reset')
        if record is None:
            if (operation!='read' or channel!=0 or offset!=0
                    or not all(b['state']=='active-idle-stopped'
                               and b.get('idle_reset_step') in (0,len(self.IDLE_RESET_SEQUENCE)) for b in self.banks)):
                return False
            self._closed_reset_ready(core)
            prior_d4=1 if self.data_history else self.STOP_ACK
            if (any(type(w) is not int or not 0<=w<=0xffffffff for w in self.registers.values())
                    or any(self.registers[base]!=1 or self.registers[base+0xd4]!=prior_d4
                           or self.registers[base+0xd8]!=1 for base in self.BASES)):
                raise UnsupportedDevice('Closed image reset requires intact quiet control/D4/D8 words')
            record=dict(cpu=core,state='write-zero',prior_words=dict(self.registers),requests=[],
                        history_sha256=self._closed_reset_history_hash())
        elif self._afs_stop_marker() is not None:self._validate_afs_stop_current()
        else:self._validate_closed_reset()
        if record['state']=='completed' and len(record['requests'])==2:
            if self._afs_preview_ack(operation,core,channel,offset,value):return True
            if self._afs_rearm_access(record,operation,core,channel,offset,value):return True
            configuration=record.get('mode_configuration')
            if (configuration is not None and configuration['state']=='applying'
                    or configuration is None and record.get('passive_reset') is None
                       and (operation,channel,offset)==('write',0,self.CONFIG_ORDER[0])):
                return self._closed_reset_configuration_access(record,operation,core,channel,offset,value)
            if self._closed_reset_mode_passive_access(record,operation,core,channel,offset,value):return True
            return self._closed_reset_passive_access(record,operation,core,channel,offset,value)
        if type(core) is not int or core!=record['cpu']:
            raise UnsupportedDevice('Closed image reset owner or paired request budget changed')
        beginning=not record['requests'] or record['state']=='completed'
        if beginning:
            request=dict(channel=len(record['requests']),state='write-zero',accesses=[])
            expected=('read',request['channel'],0,1)
        else:
            request=record['requests'][-1];state=request['state']
            if state=='write-zero':expected=('write',request['channel'],0,0)
            elif state in ('waiting-ack','acknowledged-awaiting-poll'):
                if len(request['accesses'])-2>=self.CLOSED_RESET_POLL_LIMIT:
                    raise UnsupportedDevice('Closed image reset polling budget exhausted')
                expected=('read',request['channel'],0,0 if state=='waiting-ack' else 1)
            else:expected=('write',request['channel'],0xd4,self.STOP_ACK)
        address=self.BASES[channel]+offset;word=self.registers[address] if operation=='read' else value
        if type(word) is not int or (operation,channel,offset,word)!=expected:
            raise UnsupportedDevice('Closed image reset requires its exact next paired access')
        tick=self.board.virtual_ticks
        if type(tick) is not int or not 0<=tick<1<<64:
            raise UnsupportedDevice('Closed image reset requires bounded typed time')
        if not beginning and request['state']=='write-zero':
            if type(self.stop_ticks) is not int or self.stop_ticks<1 or tick+self.stop_ticks>=1<<64:
                raise UnsupportedDevice('Closed image reset requires bounded declared delay')
            request.update(requested_tick=tick,due_tick=tick+self.stop_ticks,
                acknowledged_tick=None,acknowledged_poll_count=None)
        event=dict(operation=operation,address=address,word=word,cpu=core,tick=tick,step=len(request['accesses']))
        if operation=='write':
            old=self.registers[address];self.registers[address]=word
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old),tick=tick))
        request['accesses'].append(event)
        request['state']=('write-zero' if beginning else 'waiting-ack' if len(request['accesses'])==2
                          else 'completed' if offset==0xd4 else 'poll-observed' if word==1 else 'waiting-ack')
        if beginning:record['requests'].append(request)
        record['state']=request['state'];self.banks[0]['closed_reset']=record
        self.board.record('Closed image-bank reset access retained',**event,
            limits='One paired quiet reset; historical arm/data/config/registration unchanged; no image/IRQ/new lifetime')
        return True

    def _tick_closed_reset(self,board):
        if getattr(self,'still_reset',None) is not None:
            from . import afs_still_image_delivery
            if afs_still_image_delivery.owner(self) is not None:
                afs_still_image_delivery.tick(self,board)
            else:
                from . import image_still_reset
                image_still_reset.tick(self,board)
            return
        record=self.banks[0].get('closed_reset')
        if record is None:return
        active=('waiting-ack','acknowledged-awaiting-poll')
        if record['state'] not in active and record['requests'][-1]['state'] not in active:return
        self._validate_closed_reset();request=record['requests'][-1]
        if request['state']!='waiting-ack' or board.virtual_ticks<request['due_tick']:return
        self.registers[self.BASES[request['channel']]]=1
        request.update(acknowledged_tick=board.virtual_ticks,
            acknowledged_poll_count=len(request['accesses'])-2,state='acknowledged-awaiting-poll')
        record['state']=request['state']
        board.record('Closed image-bank bit0 acknowledgment (unmeasured)',channel=request['channel'],
            tick=board.virtual_ticks,limits='Same declared stop delay, bit0 only; no data/IRQ/registration change')

    def enable_stream(self,**options):
        from . import image_stream
        return image_stream.enable(self,**options)

    def read(self, core, address, size):
        from . import processing_image_delivery
        if processing_image_delivery.contains(self,address,size):
            return processing_image_delivery.access(self,'read',core,address,size)
        if hasattr(self, 'processing_activation'):
            from . import image_processing_activation
            return image_processing_activation.access(self,'read',core,address,size)
        channel,offset=self._offset('read',core,address,size)
        from . import processing_pipeline_reset
        producer=processing_pipeline_reset.producer_for_image(self)
        if producer is not None:
            return processing_pipeline_reset.access(producer,'read',core,address,size)
        from . import processing_passive_configuration
        producer=processing_passive_configuration.producer_for_image(self)
        if producer is not None:
            return processing_passive_configuration.access(producer,'read',core,address,size)
        if hasattr(self, 'processing_configuration'):
            from . import image_processing_configuration
            return image_processing_configuration.access(self, 'read', core, address, size)
        if hasattr(self, 'processing_reset'):
            from . import image_processing_reset
            return image_processing_reset.access(self, 'read', core, address, size)
        if self.stream is not None:
            from . import image_stream
            return image_stream.read(self,core,channel,offset)
        if self._afs_stop_marker() is None:self._validate_afs_data()
        if self._closed_reset_access('read',core,channel,offset):return self.registers[address]
        bank=self.banks[channel]
        if strict_audit.ENABLED:self._validate_passive(channel)
        value=self.registers[address]
        if type(value) is not int or not 0<=value<=0xffffffff:
            self._reject('read',core,address,size,reason='Image-bank retained value has unknown type')
        if offset==0x00:
            if value not in (0,1,0x10001):
                self._reject('read',core,address,size,reason='Unmodeled image-bank control/status state')
            if (bank['state']=='active-idle-stopped' and value==1
                    and bank['last_stop']['state']=='responded' and core==bank['last_stop']['cpu']):
                bank['last_stop'].update(state='poll-observed',poll_observed_tick=self.board.virtual_ticks)
                self.board.record('Image work idle stop poll observed',channel=channel,cpu=core,
                    control='0x1',tick=self.board.virtual_ticks,
                    effects='Actual control-bit0 read; no D4/D8, IRQ, callback or software registration effect')
        elif offset in self.ALL_CONFIG_MASKS:
            # Initial zero words are a declared fixture, even when width-minus1
            # zero could not be produced by the real aligned-width setter.
            written=any(e['address']==hex(address) for e in
                (*bank['configuration_writes'],*bank['passive_configuration_writes']))
            if written and not self._config_valid(offset,value):
                self._reject('read',core,address,size,reason='Image-bank retained configuration changed')
            if bank['state']=='active-idle-stopped' and bank['idle_reset_step'] in (1,2):
                self._idle_reset_access('read',core,channel,offset)
        return value

    def write(self, core, address, size, value):
        from . import processing_image_delivery
        if processing_image_delivery.contains(self,address,size):
            return processing_image_delivery.access(self,'write',core,address,size,value)
        if hasattr(self, 'processing_activation'):
            from . import image_processing_activation
            return image_processing_activation.access(self,'write',core,address,size,value)
        channel,offset=self._offset('write',core,address,size,value)
        from . import processing_pipeline_reset
        producer=processing_pipeline_reset.producer_for_image(self)
        if producer is not None:
            return processing_pipeline_reset.access(producer,'write',core,address,size,value)
        from . import processing_passive_configuration
        producer=processing_passive_configuration.producer_for_image(self)
        if producer is not None:
            return processing_passive_configuration.access(producer,'write',core,address,size,value)
        if hasattr(self, 'processing_configuration'):
            from . import image_processing_configuration
            return image_processing_configuration.access(self, 'write', core, address, size, value)
        if hasattr(self, 'processing_reset'):
            from . import image_processing_reset
            return image_processing_reset.access(self, 'write', core, address, size, value)
        if type(value) is not int or not 0<=value<=0xffffffff:
            self._reject('write',core,address,size,value,reason='Image-bank word exceeds32bits')
        if self.stream is not None:
            from . import image_stream
            image_stream.write(self,core,channel,offset,value)
            return
        if self._afs_stop_marker() is None:self._validate_afs_data()
        if self._closed_reset_access('write',core,channel,offset,value):return
        bank=self.banks[channel]
        self._validate_passive(channel)
        if any(b['pending_stop'] is not None for b in self.banks):
            self._reject('write',core,address,size,value,
                reason='Image-bank configuration mutation during pending stop acknowledgment')
        old=self.registers[address]
        if type(old) is not int or not 0<=old<=0xffffffff:
            self._reject('write',core,address,size,value,reason='Image-bank previous word has unknown type or range')
        event=dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=self.board.virtual_ticks)
        if offset==0x00:
            if value==0:
                if self.stop_profile not in STOP_PROFILES:
                    self._reject('write',core,address,size,value,
                        reason='Image-bank bit0 stop response requires an explicit unmeasured profile')
                initial=(old==0 and bank['state']=='initial-inactive-fixture' and not bank['stop_requests'])
                if not initial:
                    if (self.stop_profile!=IDLE_STOP_PROFILE or old!=0x10001
                            or bank['state']!='armed-awaiting-input' or bank['arm'] is None
                            or bank['registration'] is None or bank['config_step']!=len(self.CONFIG_ORDER)
                            or bank.get('idle_stop_requests',0) or type(core) is not int or core not in (0,1)):
                        self._reject('write',core,address,size,value,
                            reason='Only first inactive or explicit quiescent active image-bank stop is modeled')
                    self._idle_quiescent()
                    if any(not self._config_valid(o,self.registers[self.BASES[channel]+o])
                           for o in self.ALL_CONFIG_MASKS):
                        self._reject('write',core,address,size,value,reason='Idle image-bank stop configuration changed')
                pending=dict(cpu=core,channel=channel,request=0,old=old,
                    requested_tick=self.board.virtual_ticks,due_tick=self.board.virtual_ticks+self.stop_ticks,
                    configuration_sha256=self._config_hash(channel),state='waiting')
                if not initial:
                    pending.update(kind='active-idle',generation=bank['active_generation'])
                    bank.update(idle_stop_requests=1,idle_reset_step=0)
                bank['pending_stop']=pending
                bank['stop_requests']+=1
                bank['state']='waiting-initial-stop' if initial else 'waiting-active-idle-stop'
                self.busy=True
            elif value==0x10001:
                if (old!=1 or bank['state']!='stopped-configurable'
                        or bank['config_step']!=len(self.CONFIG_ORDER)
                        or bank['registration'] is None or bank['arm'] is not None
                        or self.registers[self.BASES[channel]+0xc0]==0):
                    self._reject('write',core,address,size,value,
                        reason='Image-bank desired arm requires complete stopped configuration and registration')
                bank['arm']=dict(channel=channel,requested_tick=self.board.virtual_ticks,
                    configuration_sha256=self._config_hash(channel),state='armed-awaiting-input',
                    desired_control=hex(value),destination=hex(self.registers[self.BASES[channel]+0xc0]),
                    logical_irq=hex(self.IRQ_CONTRACT[channel][0]),physical_irq=hex(self.IRQ_CONTRACT[channel][1]),
                    isr=hex(self.IRQ_CONTRACT[channel][2]),
                    effects='Desired request only; data,latching,completion cause and IRQ remain unresolved')
                bank['state']='armed-awaiting-input'
            else:
                self._reject('write',core,address,size,value,
                    reason='Unknown image-bank control command or active stop/completion')
        elif offset==0xd4:
            if value==1 and self.data_profile==DATA_PROFILE:
                completion=bank['pending_completion']
                if completion is None or completion['state']!='data-before-irq':
                    self._reject('write',core,address,size,value,
                        reason='Image event ACK requires its delivered virtual payload and IRQ')
                completion.update(state='acknowledged',acknowledged_tick=self.board.virtual_ticks)
                job=self.data_history[-1]
                job['outputs'][channel]['acknowledged_tick']=self.board.virtual_ticks
                if all('acknowledged_tick' in item for item in job['outputs']):
                    job['state']='acknowledged'
                bank['pending_completion']=None
                bank['state']='armed-awaiting-input'
                self.registers[address]=value;self.writes.append(event)
                self.board.record('Virtual image data acknowledged',channel=channel,**event,
                    epoch=completion['epoch'],effects='Guest D4 acknowledgment; no callback or kernel pending-bit change')
                return
            if (value!=self.STOP_ACK or bank['state']!='initial-stop-responded'
                    or old!=0 or bank['stop_acknowledgments']):
                self._reject('write',core,address,size,value,
                    reason='Only exact initial-stop D4=33FF1 acknowledgment is modeled; active eventACK is unresolved')
            bank['stop_acknowledgments']+=1
            bank['state']='stopped-configurable'
            bank['last_stop']['state']='acknowledged'
            bank['last_stop']['acknowledged_tick']=self.board.virtual_ticks
        elif offset==0xd8:
            if (value!=1 or old!=0 or bank['state']!='stopped-configurable'
                    or bank['registration'] is not None or bank['config_step']!=len(self.CONFIG_ORDER)):
                self._reject('write',core,address,size,value,
                    reason='Image-bank D8 registration requires one complete stopped configuration')
            bank['registration']=dict(channel=channel,requested_tick=self.board.virtual_ticks,
                value=1,effects='Registration selection retained; no IRQ or event is emitted')
        elif offset in self.PASSIVE_CONFIG_MASKS:
            idle=bank['state']=='active-idle-stopped'
            if ((not idle and (bank['state']!='stopped-configurable' or bank['registration'] is not None))
                    or bank['config_step']!=len(self.CONFIG_ORDER)
                    or not self._config_valid(offset,value) or bank['passive_stores']>=self.PASSIVE_CONFIG_LIMIT
                    or type(core) is not int or core not in (0,1)
                    or type(self.board.virtual_ticks) is not int or not 0<=self.board.virtual_ticks<=0xffffffffffffffff
                    or bank['passive_stores'] and self.board.virtual_ticks<bank['passive_configuration_writes'][-1]['tick']):
                self._reject('write',core,address,size,value,
                    reason='Image-bank passive configuration requires complete stopped fields before registration')
            if idle:self._idle_reset_access('write',core,channel,offset,value)
            entry={**event,'operation':'write','word':value,'old_word':old,'step':bank['passive_stores']}
            bank['passive_prior_words'].setdefault(hex(address),old)
            bank['passive_configuration_writes'].append(entry)
            bank['passive_stores']+=1;bank['passive_started']=True
            bank['passive_ledger_sha256']=self._passive_hash(bank['passive_configuration_writes'])
        else:
            if self.data_profile in (None, DATA_PROFILE) and bank['arm'] is not None:
                step=bank['active_config_step']
                if (bank['state'] not in ('armed-awaiting-input','data-delivered-awaiting-ack')
                        or offset!=self.CONFIG_ORDER[step] or not self._config_valid(offset,value)
                        or len(bank['active_configuration_writes'])>=self.ACTIVE_CONFIG_LIMIT):
                    self._reject('write',core,address,size,value,
                        reason='Virtual image next tuple requires a bounded ordered ten-field update')
                if self.data_profile is None and step == 0:
                    bank['active_configuration_writes'].clear()
                bank['active_configuration_writes'].append({**event,'step':step,
                    'generation':bank['active_generation']+1})
                bank['active_config_step']=(step+1)%len(self.CONFIG_ORDER)
                if bank['active_config_step']==0:bank['active_generation']+=1
                self.registers[address]=value;self.writes.append(event)
                self.board.record('Image next desired tuple retained',channel=channel,**event,
                    generation=bank['active_generation'],effects='Explicit next-frame latching convention; in-flight tuple unchanged')
                return
            step=bank['config_step']
            if (bank['state']!='stopped-configurable' or bank['registration'] is not None
                    or step>=len(self.CONFIG_ORDER) or offset!=self.CONFIG_ORDER[step]
                    or not self._config_valid(offset,value)):
                self._reject('write',core,address,size,value,
                    reason='Image-bank initial configuration requires the complete ordered stock setter fields/domains')
            bank['config_step']+=1
            bank['configuration_writes'].append({**event,'step':step})
        self.registers[address]=value
        self.writes.append(event)
        self.board.record('Image work bank retained write',channel=channel,state=bank['state'],**event,
            effects='Declared initial stop handshake or guest configuration/request only; no DMA,data,event IRQ,metadata or pixels')

    def tick(self, board):
        if hasattr(self, 'processing_reset'):
            from . import image_processing_reset
            image_processing_reset.tick(self, board)
            return
        now=board.virtual_ticks
        if self.stream is not None:
            from . import image_stream
            image_stream.tick(self,board)
            return
        self._tick_closed_reset(board)
        self._tick_afs_stop(board.virtual_ticks)
        for channel,bank in enumerate(self.banks):
            # The passive-configuration ledger is validated before every
            # write that could extend it; per-tick re-audits are opt-in.
            if strict_audit.ENABLED:self._validate_passive(channel)
            pending=bank['pending_stop']
            if pending is None or now<pending['due_tick']:continue
            base=self.BASES[channel]
            idle=pending.get('kind')=='active-idle'
            if (idle and (self.stop_profile!=IDLE_STOP_PROFILE
                    or bank['active_generation']!=pending['generation'])):
                raise UnsupportedDevice('Image-bank pending idle stop profile/generation changed')
            if idle:self._idle_quiescent()
            if (pending['state']!='waiting' or bank['state']!=('waiting-active-idle-stop' if idle else 'waiting-initial-stop')
                    or type(self.registers[base]) is not int or self.registers[base]!=0
                    or self._config_hash(channel)!=pending['configuration_sha256']):
                raise UnsupportedDevice('Image-bank pending stop configuration/control changed')
            self.registers[base]=1
            bank['stop_completions']+=1
            bank['state']='active-idle-stopped' if idle else 'initial-stop-responded'
            bank['last_stop']={**pending,'state':'responded','completed_tick':now,
                'inference':('Unmeasured already-idle active-bank bit0 response; no data/IRQ/callback or registration change'
                    if idle else 'Unmeasured inactive-bank clear/poll-bit0-set response only; not image/data completion')}
            bank['pending_stop']=None
            self.board.record('Image work idle stop response' if idle else 'Image work initial stop response',
                channel=channel,control='0x1',tick=now,
                inference=bank['last_stop']['inference'])
        if self.data_profile==DATA_PROFILE:
            self._tick_data(now)
            self._tick_afs_preview_data(now)
        preview=self._afs_preview_marker()
        self.dma_busy=self.pending_data is not None or preview is not None and preview['state']=='pending'
        self.busy=any(b['pending_stop'] is not None for b in self.banks) or self.dma_busy or self._afs_stop_waiting()

    def _data_route(self,channel):
        line=self.IRQ_CONTRACT[channel][1]
        controller=getattr(self.board,'interrupts',None)
        targets=None if controller is None else controller.route(line,IrqRoute.ExactlyOne)
        if targets is None:
            raise UnsupportedDevice('Virtual image requires one firmware-owned IRQ route per bank')
        return targets[0]

    def _data_tuple(self,channel):
        base=self.BASES[channel];r=self.registers
        if r[base]!=0x10001 or any(not self._config_valid(off,r[base+off]) for off in self.ALL_CONFIG_MASKS):
            raise UnsupportedDevice('Virtual image desired control or typed configuration changed')
        width,height=(r[base+0xb0]&0x7ff)+1,((r[base+0xb0]>>16)&0x7ff)+1
        stride=2*r[base+0xc4]
        source_width,source_height=(r[base+4]&0xfff)+1,((r[base+4]>>16)&0xfff)+1
        if width&1 or stride<2*width or stride*height>64*1024*1024:
            raise UnsupportedDevice('Virtual image candidate pixel geometry is invalid')
        address=r[base+0xc0];begin,end=ram_span(address,stride*height)
        return dict(channel=channel,address=hex(address),physical_begin=begin,physical_end=end,
            width=width,height=height,stride_bytes=stride,bytes=stride*height,
            source_width=source_width,source_height=source_height,
            generation=self.banks[channel]['active_generation'],route=self._data_route(channel),
            configuration_sha256=self._config_hash(channel),
            assumptions='Candidate YYCbCr; stride=2*C4; full analytic source resampling; sampled tuple at arm/update')

    def _data_source_epochs(self):
        epochs=source_epochs(self.board)
        from .histogram_protocol import CARD_HISTOGRAM_SOURCE_PROFILES, source_epoch
        inputs=[d for d in self.board.devices if type(d).__name__=='InputProcessingControl']
        if getattr(inputs[0],'event_profile',None) not in CARD_HISTOGRAM_SOURCE_PROFILES:
            return epochs
        # The card mount postpones the first paired arm until ordinal4. The
        # existing final source0 edge is its second frame, not a new IRQ edge.
        for event in inputs[0].event_history:
            if (event.get('state') not in ('asserted','acknowledged')
                    or event.get('generation')!=3 or event.get('source')!=0
                    or event.get('ordinal')!=5):
                continue
            projected={name:event.get(name) for name in ('profile','generation','ordinal',
                'attempt','cpu','source','physical_irq','asserted_tick')}|{'frame':4}
            epochs.append(source_epoch(projected))
        return epochs

    @staticmethod
    def _afs_digest(value):
        return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),
            allow_nan=False).encode()).hexdigest()

    def _afs_routes(self, *, pending=False):
        controller=getattr(self.board,'interrupts',None)
        if controller is None or len(controller.banks)!=2:
            raise UnsupportedDevice('AF-S image requires the stock paired IRQ controller')
        for channel,(_,line,isr) in enumerate(self.IRQ_CONTRACT):
            if self._data_route(channel)!=0:
                raise UnsupportedDevice('AF-S image requires CPU0 image IRQ ownership')
            bank=controller.banks[0]
            if (bank.config.get(0x400+4*line)!=isr or bank.config.get(0x800+4*line)!=10
                    or not bank.source_config&(1<<line)
                    or not pending and (bank.pending&(1<<line) or bank.active==line or bank.injected==line)):
                raise UnsupportedDevice('AF-S image IRQ registration or drain changed')

    def arm_afs_data(self, profile=AFS_DATA_PROFILE, *, frame=5):
        """Admit one existing latched pair for an explicit sensor source5/6.

        A delayed admission may use an already asserted edge only when both
        complete tuples predate it. The modeled delay begins at admission;
        this neither creates another source edge nor estimates sensor timing.
        """
        from .afs_frame_source import requests
        if (profile!=AFS_DATA_PROFILE or type(frame) is not int or frame not in (5,6)
                or self.data_profile!=DATA_PROFILE or self.banks[0].get('afs_data') is not None):
            raise UnsupportedDevice('Unknown or repeated AF-S image admission')
        self._idle_quiescent()
        if (len(self.data_history)!=2 or any(b['state']!='armed-awaiting-input'
                or b['arm'] is None or b['registration'] is None
                or b['active_generation']!=2 or b['active_config_step']
                or len(b['active_configuration_writes'])!=2*len(self.CONFIG_ORDER)
                for b in self.banks)):
            raise UnsupportedDevice('AF-S image requires two drained jobs and complete generation2 tuples')
        from . import afs_startup_scope
        inputs=[d for d in self.board.devices if type(d).__name__=='InputProcessingControl']
        if len(inputs)!=1:
            raise UnsupportedDevice('AF-S image requires one selected REC startup input')
        frame_offset=afs_startup_scope.scope(inputs[0].event_profile)['ordinal_offset']
        if ([job['epoch'].get('frame') for job in self.data_history]!=[2+frame_offset,3+frame_offset]
                or any(len(job['outputs'])!=2 or any(item['generation']!=index
                    for item in job['outputs']) for index,job in enumerate(self.data_history))):
            raise UnsupportedDevice('AF-S image prior pairs differ from the bounded generation0/1 prefix')
        markers=[r for r in requests(self.board) if r['frame']==frame and r['state']!='canceled']
        if len(markers)!=1:
            raise UnsupportedDevice('AF-S image requires its explicitly armed source identity')
        source=markers[0]
        boundary=source['asserted_tick'] if source['asserted_tick'] is not None else source['due_tick']
        for channel,bank in enumerate(self.banks):
            previous=-1
            for index,event in enumerate(bank['active_configuration_writes']):
                step=index%len(self.CONFIG_ORDER);address=self.BASES[channel]+self.CONFIG_ORDER[step]
                if (event.get('step')!=step or event.get('generation')!=1+index//len(self.CONFIG_ORDER)
                        or event.get('address')!=hex(address) or event.get('cpu')!=0
                        or type(event.get('tick')) is not int or not previous<=event['tick']<=boundary
                        or index>=len(self.CONFIG_ORDER) and event.get('value')!=hex(self.registers[address])):
                    raise UnsupportedDevice('AF-S image tuple ledger/order/words differ or postdate its source')
                previous=event['tick']
        self._afs_routes()
        outputs=[self._data_tuple(channel) for channel in range(2)]
        if any(item['bytes']>4*1024*1024 for item in outputs):
            raise UnsupportedDevice('AF-S image output exceeds the bounded scene size')
        if outputs[0]['physical_begin']<outputs[1]['physical_end'] and outputs[1]['physical_begin']<outputs[0]['physical_end']:
            raise UnsupportedDevice('AF-S image latched output spans overlap')
        now=self.board.virtual_ticks
        if type(now) is not int or not 0<=now<1<<64 or now+self.data_ticks>=1<<64:
            raise UnsupportedDevice('AF-S image admission delay exceeds the clock bound')
        immutable=('profile','generation','ordinal','attempt','frame','cpu','source','physical_irq',
            'ack_selector','requested_tick','due_tick','delay_ticks','prefix_sha256','producer')
        marker=dict(profile=profile,frame=frame,admitted_tick=now,data_ticks=self.data_ticks,
            input_sha256=self._afs_digest({key:source[key] for key in immutable}),
            prior_history_sha256=self._afs_digest(self.data_history),outputs=outputs,
            tuple_ledger_sha256=self._afs_digest([b['active_configuration_writes'] for b in self.banks]),
            assumptions='One declared generation2 post-ISP YYCbCr pair; actual source identity, immutable tuple and guest D4/IRQ clients; unmeasured admission delay and ISP law')
        marker['sha256']=self._afs_digest(marker)
        self.banks[0]['afs_data']=marker
        self.board.record('AF-S latched image pair admitted',**marker)
        return deepcopy(marker)

    def _validate_afs_data(self):
        marker=self.banks[0].get('afs_data')
        if marker is None:return None
        from .afs_frame_source import requests,epoch_for
        if (type(marker) is not dict or marker.get('profile')!=AFS_DATA_PROFILE
                or marker.get('sha256')!=self._afs_digest({k:v for k,v in marker.items() if k!='sha256'})
                or marker.get('data_ticks')!=self.data_ticks or len(self.data_history) not in (2,3)
                or type(marker.get('outputs')) is not list or len(marker['outputs'])!=2
                or marker.get('frame') not in (5,6)
                or type(marker.get('admitted_tick')) is not int
                or not 0<=marker['admitted_tick']<=self.board.virtual_ticks
                or marker.get('tuple_ledger_sha256')!=self._afs_digest([b['active_configuration_writes'] for b in self.banks])
                or marker.get('prior_history_sha256')!=self._afs_digest(self.data_history[:2])):
            raise UnsupportedDevice('AF-S image admission or original job history changed')
        sources=[r for r in requests(self.board) if r['frame']==marker['frame'] and r['state']!='canceled']
        immutable=('profile','generation','ordinal','attempt','frame','cpu','source','physical_irq',
            'ack_selector','requested_tick','due_tick','delay_ticks','prefix_sha256','producer')
        if (len(sources)!=1 or marker['input_sha256']!=self._afs_digest({key:sources[0][key] for key in immutable})):
            raise UnsupportedDevice('AF-S image lost its real source identity')
        job=self.pending_data if self.pending_data is not None else self.data_history[2] if len(self.data_history)==3 else None
        if job is not None:
            if (job.get('afs_admission_sha256')!=marker['sha256']
                    or job['epoch']!=epoch_for(self.board,marker['frame'])
                    or len(job['outputs'])!=2
                    or [{key:item[key] for key in wanted} for item,wanted in zip(job['outputs'],marker['outputs'])]!=marker['outputs']
                    or job['requested_tick']<marker['admitted_tick']
                    or job['due_tick']!=job['requested_tick']+self.data_ticks):
                raise UnsupportedDevice('AF-S image pending/completed job differs from its latched admission')
        return marker

    def _tick_data(self,now):
        # The bounded third AF pair has no further DMA or timer once both
        # guest D4 ACKs completed. Its immutable admission is checked again
        # at MMIO/snapshot consumption, rather than on every idle quantum.
        marker=self.banks[0].get('afs_data')
        if (isinstance(marker,dict) and marker.get('profile')==AFS_DATA_PROFILE
                and self.pending_data is None and len(self.data_history)==3
                and self.data_history[-1].get('state')=='acknowledged'
                and len(self.data_history[-1].get('outputs',()))==2
                and all('acknowledged_tick' in item for item in self.data_history[-1]['outputs'])
                and all(bank['pending_completion'] is None for bank in self.banks)):
            return
        afs=self._validate_afs_data()
        pending=self.pending_data
        if pending is not None and now>=pending['due_tick']:
            if afs is not None:
                self._afs_routes()
                if any(self._config_hash(item['channel'])!=item['configuration_sha256'] for item in pending['outputs']):
                    raise UnsupportedDevice('AF-S image latched configuration changed before payload delivery')
            from x70_native._native import virtual_scene_yycbcr
            payloads=[]
            for item in pending['outputs']:
                begin,end=ram_span(int(item['address'],16),item['bytes'])
                if (begin,end)!=(item['physical_begin'],item['physical_end']):
                    raise UnsupportedDevice('Virtual image sampled DRAM span changed')
                if self._data_route(item['channel'])!=item['route']:
                    raise UnsupportedDevice('Virtual image IRQ route changed while payload pending')
                data=virtual_scene_yycbcr(item['width'],item['height'],item['stride_bytes'],
                    pending['epoch']['frame'],item['source_width'],item['source_height'],
                    0,0,item['source_width'],item['source_height'])
                if len(data)!=item['bytes']:
                    raise UnsupportedDevice('Native virtual image length differs from sampled span')
                payloads.append(data)
            # Validate both spans/generator outputs before the first bus write.
            for item,data in zip(pending['outputs'],payloads):
                self.board.write_memory(int(item['address'],16),data)
                item['payload_sha256']=hashlib.sha256(data).hexdigest()
            pending.update(state='data-before-irq',completed_tick=now)
            for item in pending['outputs']:
                channel=item['channel'];bank=self.banks[channel]
                bank['pending_completion']=dict(state='data-before-irq',epoch=dict(pending['epoch']),
                    payload_sha256=item['payload_sha256'],completed_tick=now)
                bank['state']='data-delivered-awaiting-ack'
                self.board.queue_irq(item['route'],self.IRQ_CONTRACT[channel][1])
            self.data_history.append(pending);self.pending_data=None
            self.board.record('Virtual image payloads delivered before IRQ',epoch=pending['epoch'],outputs=pending['outputs'])
        if self.pending_data is not None or len(self.data_history)>=(3 if afs is not None else 2):
            return
        if any(b['state']!='armed-awaiting-input' or b['arm'] is None
               or b['pending_completion'] is not None or b['active_config_step'] for b in self.banks):
            return
        if afs is not None and len(self.data_history)==2:
            from .afs_frame_source import epoch_for
            observed=epoch_for(self.board,afs['frame'])
            epochs=[observed] if observed is not None else []
        else:epochs=self._data_source_epochs()
        if not epochs:return
        epoch=epochs[-1]
        if self.data_history:
            last=self.data_history[-1]
            if epoch_key(epoch)==epoch_key(last['epoch']):return
            if any(b['active_generation']<=item['generation'] for b,item in zip(self.banks,last['outputs'])):
                return
        outputs=deepcopy(afs['outputs']) if afs is not None and len(self.data_history)==2 else [self._data_tuple(channel) for channel in range(2)]
        if afs is not None and any(self._config_hash(item['channel'])!=item['configuration_sha256'] for item in outputs):
            raise UnsupportedDevice('AF-S image desired tuple changed before its source admission')
        if outputs[0]['physical_begin']<outputs[1]['physical_end'] and outputs[1]['physical_begin']<outputs[0]['physical_end']:
            raise UnsupportedDevice('Virtual image output spans overlap')
        self.pending_data=dict(state='sampled-awaiting-data',epoch=dict(epoch),outputs=outputs,
            requested_tick=now,due_tick=now+self.data_ticks,
            assumptions='Declared post-ISP source and unmeasured delay/latching; no silicon ISP or frame clock claim')
        if afs is not None:self.pending_data['afs_admission_sha256']=afs['sha256']
        self.board.record('Virtual image tuple sampled',**self.pending_data)

    def next_event_tick(self, now):
        if hasattr(self, 'processing_reset'):
            from . import image_processing_reset
            return image_processing_reset.due(self)
        due=[b['pending_stop']['due_tick'] for b in self.banks if b['pending_stop'] is not None]
        if self.stream is not None:
            from . import image_stream
            stream_due=image_stream.due(self)
            if stream_due is not None:due.append(stream_due)
        if getattr(self,'still_reset',None) is not None:
            from . import image_still_reset
            if image_still_reset.due(self) is not None:due.append(image_still_reset.due(self))
            from . import afs_still_image_delivery
            if afs_still_image_delivery.due(self) is not None:due.append(afs_still_image_delivery.due(self))
        record=self.banks[0].get('closed_reset')
        if record is not None and record['state']=='waiting-ack':due.append(record['requests'][-1]['due_tick'])
        close=self._afs_stop_marker()
        if close is not None and close['state']=='waiting-ack':due.append(close['requests'][-1]['due_tick'])
        if self.pending_data is not None:due.append(self.pending_data['due_tick'])
        preview=self._afs_preview_marker()
        if preview is not None and preview['state']=='pending':due.append(preview['due_tick'])
        return min(due) if due else None

    def snapshot(self):
        if hasattr(self, 'processing_activation'):
            from . import image_processing_activation
            result = type(self).snapshot(image_processing_activation.historical(self))
            result['processing_activation'] = image_processing_activation.summary(self)
            return result
        if self.stream is None:
            self._validate_afs_data()
            self._validate_closed_reset()
        result = dict(name='ImageWorkControl',bases=[hex(a) for a in self.BASES],
            stream=self.stream,
            stop_profile=self.stop_profile,stop_ticks=self.stop_ticks,busy=self.busy,
            data_profile=self.data_profile,data_ticks=self.data_ticks,dma_busy=self.dma_busy,
            pending_data=self.pending_data,data_history=self.data_history,
            registers={hex(a):hex(v) for a,v in sorted(self.registers.items())},
            banks=self.banks,writes=self.writes,last_rejected=self.last_rejected,
            limits=([
                'Continuous firmware-programmed image banks latch destinations and geometry on virtual source edges; YYCbCr bytes are DMA-written before IRQ2C/2D and require guest D4 acknowledgment.',
                'Stop-bit0 latency, source cadence, tuple latching, pixel packing, color conversion and sampling are declared unmeasured laws.',
                'Synthetic post-ISP imagery does not establish sensor, optics, autofocus, captured-frame equivalence or physical timing.'
            ] if self.stream is not None else [
                'Initial zero controls/configuration and optional stop-bit0 response/latency are unmeasured.',
                ('The stop is bounded to each first inactive bank; later active quiescence/completion rejects.'
                 if self.stop_profile!=IDLE_STOP_PROFILE else
                 'One additional idle active stop per bank requires all modeled DMA/completions/tuple updates drained; control-bit0 response/latency are unmeasured. The real poll observes readiness; no active D4/D8 acknowledgment or IRQ is supplied. Only the exact 34-store passive reset recipe is accepted afterward; arm/data/registration histories remain retained.'),
                'After both completed quiet closes, with either neither or both passive recipes already applied, one1201F1C/92A8D0 paired R1/W0/poll-bit0/D4=33FF1 transaction may use the same declared stop delay. One subsequent1203540(mask300)/10CA754 paired passive recipe is accepted. A separate plain ledger preserves every preceding bank/arm/data/registration/configuration record, including equal-tick poll order; it supplies no new image, IRQ or processing lifetime.',
                'One explicit AF11 initializer-source0 admission permits exactly D8=1/read-control1/write10001 per bank after the completed paired mode setters and reset. New desired arms retain separate plain identities while all preceding banks/data/registrations stay immutable; no further DMA, completion status or IRQ is supplied.',
                'Driver-constructed fields and desired arm are retained without predicting hardware latching or data.',
                'At most256 pure passive stores per stopped bank retain10CB618 initial35-word and10CA754 reset34-word fields before registration. Order is recorded; these stores have no completion semantics or imagecontrol effects.',
                ('No DMA payload,image,source/result status,IRQ,callback,metadata,display frame or REC readiness is supplied.'
                 if self.data_profile is None else
                'At most two startup virtual post-ISP YYCbCr pairs are written before peripheral IRQ2C/2D; actual firmware ACK/callback paths run. Only the explicit late-card input profile additionally projects its already asserted generation3/source0 ordinal5 as frame4 after a fresh ten-field update. An explicit AF-S admission allows exactly one further generation2 pair, tied to actual independent source5/6 and pre-source ordered setter fields; its original two jobs stay immutable. Source clock, stride=2*C4, full-source resampling, tuple latching and admission delay are unmeasured conventions. No callback, queue, metadata or REC readiness is fabricated.')]))
        if hasattr(self, 'processing_reset'):
            from . import image_processing_reset
            result['processing_reset'] = image_processing_reset.summary(self)
        if hasattr(self, 'processing_configuration'):
            from . import image_processing_configuration
            result['processing_configuration'] = image_processing_configuration.summary(self)
        from . import processing_pipeline_reset
        producer=processing_pipeline_reset.producer_for_image(self)
        if producer is not None:
            result['processing_pipeline_reset'] = processing_pipeline_reset.summary(producer)
        return result

    def arm_processing_reset(self, profile):
        from . import image_processing_reset
        return image_processing_reset.arm(self, profile)

    def arm_processing_configuration(self, profile, *, caller_address=None):
        from . import image_processing_configuration
        return image_processing_configuration.arm(self, profile, caller_address=caller_address)

    def processing_configuration_summary(self):
        from . import image_processing_configuration
        return image_processing_configuration.summary(self)

    def arm_processing_activation(self, profile, *, channel=0, caller_address=None):
        from . import image_processing_activation
        return image_processing_activation.arm(self, profile, channel=channel, caller_address=caller_address)

    def processing_activation_summary(self):
        from . import image_processing_activation
        return image_processing_activation.summary(self)
