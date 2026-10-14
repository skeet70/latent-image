"""Exact stock FE80 reset/program-upload/enable and companion route setup.

Only the four known static descriptors and their unchanged program hashes are
accepted. Programs are retained as opaque data, never executed. An opt-in
first AF preview request accepts its exact start transaction. Numerical output
remains unknown. A separately selected stock-cancelled job may deliver physical
control completion and its exact retirement, without changing result RAM.
"""
import hashlib
import json
import struct

from .hardware import UnsupportedDevice


PREVIEW_START_PROFILE='declared-afs-first-fe80-preview-start-opaque-unmeasured'
CANCELLED_COMPLETION_PROFILE='declared-afs-cancelled-fe80-control-completion-unmeasured'
#91E7EC's three-instruction poll may run through a full1000-instruction
#native slice before the board services its due physical enable response.
#512 reads accommodate that bounded slice plus the ready read; they do not
#alter the delay, response ownership or accepted control sequence.
CANCELLED_ENABLE_POLL_LIMIT=512


class FE80Control:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    stream=None  # Plain state of the opt-in continuous FE80 stream (class default keeps old checkpoints loadable).
    PAGE_RANGES=((0xfe318000,0x1000),(0xfe3b0000,0x1000),
                 (0xfe80c000,0x1000),(0xfe80d000,0x1000))
    OFFSETS={0xfe80d840,0xfe80d844,0xfe80d848,0xfe80d84c,0xfe80d800,
             0xfe80cc20,0xfe80c540,0xfe80c1c8,0xfe80c1c0,0xfe80c000,
             0xfe3b0000,0xfe318040,0xfe318048,0xfe318010,0xfe318008}
    PROGRAMS={
        0xbdf870:dict(period=12,descriptor='000000038072b500e048000000000000',
            source=0xb57280,bytes=18656,sha256='34131494b53051b95fe32c3a672fe606a5f0eed75d85f1533eb5a25ff1f0db18'),
        0xbdf880:dict(period=13,descriptor='00000003c0e9ad00406e000000000000',
            source=0xade9c0,bytes=28224,sha256='72476fe1dbc845ca8c8877ac24da97a44d8bba3264038d1b335282795729fd8d'),
        0xbdf890:dict(period=16,descriptor='0000000350ebac0068fe000000000000',
            source=0xaceb50,bytes=65128,sha256='3a9118e784fc775a22831b0e97cd1167e541f03380cea8a95406b64c7a120b25'),
        0xb57270:dict(period=12,descriptor='0000000370c6ab00d824010000000000',
            source=0xabc670,bytes=74968,sha256='e6c8c3095d920a4f0a1ec055ed5d801f4ddbd39af5535d51fefe7f84f50c4ce8'),
    }
    ROUTE_SEQUENCE=((0xfe318040,1),(0xfe318040,0),(0xfe318048,1),
                    (0xfe318010,0),(0xfe318008,1))
    PREVIEW_OFFSETS={0xfe80c440,0xfe80c108,0xfe80c480,0xfe80c488,
                     0xfe80c404,0xfe80c840,0xfe80c848,0xfe80c804,
                     0xfe80c100,0xfe80d804,0xfe80c400,0xfe80c800}
    PREVIEW_START=(('write',0xfe80c440,0x40001),('write',0xfe80c000,1),
        ('write',0xfe80c108,0x40bdf8b0),('write',0xfe80c480,0x40101),
        ('write',0xfe80c488,0),('read',0xfe80c404,0),('write',0xfe80c404,0),
        ('write',0xfe80c840,0x40101),('write',0xfe80c848,0),
        ('read',0xfe80c804,0),('write',0xfe80c804,0),('write',0xfe80c100,1))
    PREVIEW_DESCRIPTOR_SHA256='054e4f0cbc411c8b1eef807635eff0a2f292fabbf6a37ea4b8b9a1cb0190bda4'
    PREVIEW_SECONDARY_SHA256='93f29b3de94d3e607b297f885b6f0133cec25a1c735bacc28ca5f31ea89da35f'
    CANCELLED_RETIREMENT=(('read',0xfe80d804,0),('write',0xfe318040,1),
        ('write',0xfe318040,0),('write',0xfe318048,1),('write',0xfe318010,0),
        ('write',0xfe318008,1),('write',0xfe80c100,0),('read',0xfe80c100,0),
        ('write',0xfe80c100,0x100),('write',0xfe80c440,0x40000),
        ('read',0xfe80c440,0x40000),('write',0xfe80c480,0x40100),
        ('read',0xfe80c480,0x40100),('read',0xfe80c404,0),('write',0xfe80c404,2),
        ('write',0xfe80c840,0x40100),('read',0xfe80c840,0x40100),
        ('read',0xfe80c804,0),('write',0xfe80c804,1),
        ('write',0xfe80d800,0x1ff),('write',0xfe80d844,1))

    UPLOAD_SETUP_PHASES = frozenset(('routed',))
    PERIOD_VALUES = frozenset((0xb0000,0xc0000,0xf0000))

    def __init__(self, board, *, handshake_ticks=2, upload_bytes_per_tick=256, programs=None):
        if isinstance(handshake_ticks,bool) or not isinstance(handshake_ticks,int) or handshake_ticks<1 or isinstance(upload_bytes_per_tick,bool) or not isinstance(upload_bytes_per_tick,int) or upload_bytes_per_tick<1:
            raise ValueError('FE80 handshake delay and upload throughput must be positive integers')
        if programs is not None:
            from .fe80_contracts import FE80Program
            if (type(programs) is not tuple or
                    any(not isinstance(row,FE80Program) for row in programs) or
                    len({row.descriptor.address for row in programs}) != len(programs)):
                raise ValueError('FE80 programs require unique immutable firmware contracts')
            self.PROGRAMS = {row.descriptor.address:dict(period=row.period,
                descriptor=row.descriptor.data.hex(),source=row.program.address,
                bytes=len(row.program.data),sha256=row.program.sha256) for row in programs}
            for row in programs:
                if row.setup_phase != 'routed':self.PROGRAMS[row.descriptor.address]['setup_phase']=row.setup_phase
            phases=frozenset(row.setup_phase for row in programs)
            periods=frozenset((row.period-1)<<16 for row in programs)
            if programs and phases!=self.UPLOAD_SETUP_PHASES:self.UPLOAD_SETUP_PHASES=phases
            if programs and periods!=self.PERIOD_VALUES:self.PERIOD_VALUES=periods
        self.board=board
        self.handshake_ticks,self.upload_bytes_per_tick=handshake_ticks,upload_bytes_per_tick
        self.registers={};self.phase='idle';self.upload_phase='idle';self.owner=None
        self.busy=False;self.uploaded=False;self.route_index=None;self.route_ready=False
        self.handshakes_started=self.handshakes_completed=self.uploads_started=self.uploads_completed=0
        self.last_upload=None;self.program_data=b''
        board.record('FE80 controller explicit reset fixture',
            limit='Only known setup/program upload acknowledgement; no engine execution or output effects')

    def enable_stream(self,**options):
        from . import fe80_stream
        return fe80_stream.enable(self,**options)

    def contains(self,address,size):
        if self.stream is not None:
            from . import fe80_stream
            return size==4 and fe80_stream.in_pages(address)
        return size==4 and (address in self.OFFSETS or
            self._preview_runtime() is not None and address in self.PREVIEW_OFFSETS)

    def _preview_runtime(self):
        return self.last_upload.get('preview_runtime') if isinstance(self.last_upload,dict) else None

    @staticmethod
    def _seal(value):
        return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()

    def _preview_image(self):
        images=[device for device in self.board.devices if type(device).__name__=='ImageWorkControl']
        if len(images)!=1:
            raise UnsupportedDevice('FE80 preview needs one acknowledged image-output owner')
        image=images[0]
        data=image.banks[0].get('closed_reset',{}).get('af_rearm',{}).get('preview_data')
        if not isinstance(data,dict) or data.get('state')!='acknowledged':
            raise UnsupportedDevice('FE80 preview image pair is not stock-acknowledged')
        latch=data.get('latch',{});outputs=latch.get('outputs',())
        hashes=data.get('payload_sha256',())
        if (len(outputs)!=2 or len(hashes)!=2 or len(data.get('acknowledgments',()))!=2
                or any(item is None for item in data['acknowledgments'])):
            raise UnsupportedDevice('FE80 preview image-output identity is incomplete')
        output=outputs[0]
        if (int(output.get('address','0'),16)&0x3fffffff!=0x05cd05c0 or
                (output.get('width'),output.get('height'),output.get('stride_bytes'),output.get('bytes'))
                !=(2048,208,4096,851968)):
            raise UnsupportedDevice('FE80 preview input layout differs from the qualified first output')
        return data,output,hashes[0]

    def arm_preview_processing(self,profile=PREVIEW_START_PROFILE):
        """Select the actual first AF preview start; supply no processing result.

        The immutable input and exact control transaction are retained under
        the existing upload record. The uploaded engine code remains opaque;
        result reads, retirement and further requests stay unsupported.
        """
        if (profile!=PREVIEW_START_PROFILE or self._preview_runtime() is not None
                or self.owner!=0 or self.busy or not self.uploaded or not self.route_ready
                or self.phase!='routed' or self.upload_phase!='released'):
            raise UnsupportedDevice('FE80 preview start admission is not the qualified routed idle owner')
        known=self.PROGRAMS.get(0xb57270)
        if known is None:
            raise UnsupportedDevice('Selected FE80 programs do not qualify this legacy preview recipe')
        if (not isinstance(self.last_upload,dict) or not self.last_upload.get('completed')
                or self.last_upload.get('descriptor_physical')!='0xb57270'
                or self.last_upload.get('sha256')!=known['sha256']
                or self.last_upload.get('bytes')!=known['bytes']
                or hashlib.sha256(self.program_data).hexdigest()!=known['sha256']):
            raise UnsupportedDevice('FE80 preview requires the exact unchanged uploaded program')
        read=lambda address,size:bytes(self.board.read_memory(address,size,cpu_id=0))
        value=lambda address,size=4:int.from_bytes(read(address,size),'little')
        if (value(0x160d134)!=1 or value(0x160d138,1)!=1 or value(0x160d139,1)!=0
                or value(0x1625fc0)!=2 or value(0x15fcd74)!=0x1245bf8
                or value(0x15e3d48)!=1 or value(0x15d5360)!=1):
            raise UnsupportedDevice('FE80 preview actual first request/callback state differs')
        data,output,payload_hash=self._preview_image()
        descriptor=read(0x40bdf8b0,128);secondary=read(0x40bdf7a0,80)
        if (descriptor[:32]!=struct.pack('<8I',0x03000207,0x40bdf7f0,44,0,
                                      0x03000207,0x40bdf7a0,80,0)
                or hashlib.sha256(descriptor).hexdigest()!=self.PREVIEW_DESCRIPTOR_SHA256
                or hashlib.sha256(secondary).hexdigest()!=self.PREVIEW_SECONDARY_SHA256):
            raise UnsupportedDevice('FE80 preview parameter descriptors differ')
        parameters=read(0x40bdf7f0,44)
        fields=struct.unpack('<11I',parameters)
        if (fields[0]!=0x45cd05c0 or fields[1]!=0x5ff235d4
                or fields[2]!=0x080000d0 or fields[3]!=36
                or fields[4:8]!=(0x04020202,0x000e0e21,0x04020202,0x000e0e21)
                or fields[8]!=0x001e0058 or fields[9]!=0x00ca0398
                or fields[10]!=0x15d80b0 or value(0x15d80a8)!=fields[1]
                or value(0x15d80ac)!=fields[10]):
            raise UnsupportedDevice('FE80 preview parameters differ from the actual first request')
        source=read(fields[0],output['bytes'])
        if hashlib.sha256(source).hexdigest()!=payload_hash:
            raise UnsupportedDevice('FE80 preview source bytes differ from acknowledged image output')
        latch=dict(profile=profile,cpu=0,armed_tick=self.board.virtual_ticks,
            upload_sha256=self._seal(self.last_upload),image_sha256=self._seal(data),
            input_address=fields[0],input_bytes=len(source),input_sha256=payload_hash,
            output_address=fields[1],parameters_hex=parameters.hex(),
            secondary_hex=secondary.hex(),descriptor_hex=descriptor.hex(),
            tone_lut_sha256=hashlib.sha256(read(fields[10],256)).hexdigest(),
            registers={hex(address):value for address,value in sorted(self.registers.items())})
        self.last_upload['preview_runtime']=dict(state='armed',latch=latch,
            latch_sha256=self._seal(latch),events=[])
        self.board.record('FE80 declared first preview processing start armed',profile=profile,
            input_sha256=payload_hash,limit='Opaque engine program; no output, completion status or IRQ supplied')

    def _validate_preview_runtime(self):
        runtime=self._preview_runtime()
        if runtime is None:return
        cancel=runtime.get('cancelled_completion')
        if (set(runtime)!= {'state','latch','latch_sha256','events'} | ({'cancelled_completion'} if cancel is not None else set())
                or runtime['state'] not in ('armed','starting','awaiting-result')
                or not isinstance(runtime['latch'],dict)
                or self._seal(runtime['latch'])!=runtime['latch_sha256']):
            raise UnsupportedDevice('FE80 preview retained request is malformed or changed')
        latch=runtime['latch'];events=runtime['events'];known=self.PROGRAMS[0xb57270]
        if (set(latch)!= {'profile','cpu','armed_tick','upload_sha256','image_sha256',
                'input_address','input_bytes','input_sha256','output_address','parameters_hex',
                'secondary_hex','descriptor_hex','tone_lut_sha256','registers'}
                or type(latch['armed_tick']) is not int or not 0<=latch['armed_tick']<=self.board.virtual_ticks
                or (latch['input_address'],latch['input_bytes'],latch['output_address'])
                !=(0x45cd05c0,851968,0x5ff235d4)):
            raise UnsupportedDevice('FE80 preview retained latch fields are outside the qualified request')
        if (latch.get('profile')!=PREVIEW_START_PROFILE or latch.get('cpu')!=0
                or self.owner!=0 or self.phase not in ('routed','enable-requested','enable-pending','enabled')
                or cancel is None and (self.phase!='routed' or self.busy)
                or self.upload_phase!='released' or not self.uploaded or not self.route_ready
                or hashlib.sha256(self.program_data).hexdigest()!=known['sha256']
                or self._seal({key:value for key,value in self.last_upload.items() if key!='preview_runtime'})
                !=latch['upload_sha256']):
            raise UnsupportedDevice('FE80 preview original program/setup ownership changed')
        if not isinstance(events,list) or len(events)>len(self.PREVIEW_START):
            raise UnsupportedDevice('FE80 preview control transaction bound changed')
        expected_state='armed' if not events else 'awaiting-result' if len(events)==12 else 'starting'
        if runtime['state']!=expected_state:
            raise UnsupportedDevice('FE80 preview state does not match accepted start accesses')
        projection={int(address,16):value for address,value in latch['registers'].items()}
        previous=latch['armed_tick']
        for item,(operation,address,value) in zip(events,self.PREVIEW_START):
            if (set(item)!= {'operation','address','value','tick','cpu'}
                    or (item['operation'],item['address'],item['value'],item['cpu'])
                    !=(operation,address,value,0) or type(item['tick']) is not int
                    or not previous<=item['tick']<=self.board.virtual_ticks):
                raise UnsupportedDevice('FE80 preview control ledger differs from the stock transaction')
            previous=item['tick']
            if operation=='write':projection[address]=0 if address==0xfe80c000 else value
        if cancel is not None:self._validate_cancelled_completion(cancel,projection)
        if projection!=self.registers:
            raise UnsupportedDevice('FE80 preview register projection changed')
        read=lambda address,size:bytes(self.board.read_memory(address,size,cpu_id=0))
        if (read(0x40bdf8b0,128).hex()!=latch['descriptor_hex']
                or read(0x40bdf7f0,44).hex()!=latch['parameters_hex']
                or read(0x40bdf7a0,80).hex()!=latch['secondary_hex']
                or hashlib.sha256(read(0x15d80b0,256)).hexdigest()!=latch['tone_lut_sha256']):
            raise UnsupportedDevice('FE80 preview live descriptor/parameter/LUT bytes changed')
        data,output,payload_hash=self._preview_image()
        if (self._seal(data)!=latch['image_sha256'] or payload_hash!=latch['input_sha256']
                or hashlib.sha256(read(latch['input_address'],latch['input_bytes'])).hexdigest()!=payload_hash):
            raise UnsupportedDevice('FE80 preview acknowledged source ownership or bytes changed')

    def _cancelled_route(self):
        controller=getattr(self.board,'interrupts',None)
        if controller is None or len(controller.banks)!=2:
            raise UnsupportedDevice('Cancelled FE80 completion needs the stock interrupt controller')
        bank,other=controller.banks
        read=lambda address:int.from_bytes(self.board.read_memory(address,4,cpu_id=0),'little')
        handle=read(0xc6dbb4+0x27*4)
        config=(bank.config.get(0x400+4*88),bank.config.get(0x800+4*88),
                bank.config.get(0x180+4*(88//16),0)>>(2*(88%16))&3)
        if (config!=(0x13372e8,13,1) or bank.source_config&(1<<88) or not bank.enabled&(1<<88)
                or (bank.pending|other.pending|other.enabled)&(1<<88)
                or any(b.active==88 or b.injected==88 or any(i['line']==88 for i in b.active_stack)
                       for b in controller.banks)
                or handle==0 or handle!=read(0xc96f20+88*4)):
            raise UnsupportedDevice('Cancelled FE80 completion route/handle is not the idle CPU0 IRQ88 registration')
        return dict(cpu=0,line=88,vector=0x13372e8,priority=13,trigger_mode=1,source_type=0,handle=handle)

    def _cancelled_flags(self):
        return bytes(self.board.read_memory(0x160d134,6,cpu_id=0))

    def arm_cancelled_completion(self,profile=CANCELLED_COMPLETION_PROFILE,*,delay_ticks=1000):
        """Complete only the stock discarded-result branch through physical IRQ88.

        The caller has already requested cancellation:134=2,138=0,139=1.
        No opaque-program numerical output is produced or qualified. The
        real ISR must unregister its IRQ, retire controls, skip its saved
        numerical callback, re-enable hardware and release the kernel wait.
        """
        self._validate_preview_runtime();runtime=self._preview_runtime()
        if (profile!=CANCELLED_COMPLETION_PROFILE or type(delay_ticks) is not int
                or not 1<=delay_ticks<=1000000 or runtime is None
                or runtime['state']!='awaiting-result' or 'cancelled_completion' in runtime
                or self._cancelled_flags()!=bytes.fromhex('020000000001')
                or int.from_bytes(self.board.read_memory(0x15fcd74,4,cpu_id=0),'little')!=0x1245bf8):
            raise UnsupportedDevice('FE80 discarded completion requires the actual cancelled first-job state')
        route=self._cancelled_route();now=self.board.virtual_ticks
        latch=dict(profile=profile,armed_tick=now,delay_ticks=delay_ticks,due_tick=now+delay_ticks,
            route=route,output_sha256=hashlib.sha256(self.board.read_memory(0x5ff235d4,88,cpu_id=0)).hexdigest(),
            handshakes_started=self.handshakes_started,handshakes_completed=self.handshakes_completed)
        runtime['cancelled_completion']=dict(state='armed',latch=latch,latch_sha256=self._seal(latch),
            events=[],asserted_tick=None,enable_due_tick=None,enable_ready_tick=None,enable_ready_poll_count=None)
        self.board.record('FE80 discarded-result control completion armed',profile=profile,
            delay_ticks=delay_ticks,limit='No numerical output modeled; only the already-cancelled stock IRQ88 cleanup branch')

    def _validate_cancelled_completion(self,cancel,projection):
        if (not isinstance(cancel,dict) or set(cancel)-{'enable_ready_poll_count'}!= {'state','latch','latch_sha256','events',
                'asserted_tick','enable_due_tick','enable_ready_tick'}
                or not isinstance(cancel['latch'],dict) or self._seal(cancel['latch'])!=cancel['latch_sha256']):
            raise UnsupportedDevice('FE80 cancelled completion retained identity changed')
        latch=cancel['latch'];events=cancel['events'];now=self.board.virtual_ticks
        if (set(latch)!= {'profile','armed_tick','delay_ticks','due_tick','route','output_sha256',
                'handshakes_started','handshakes_completed'}
                or latch['profile']!=CANCELLED_COMPLETION_PROFILE
                or type(latch['armed_tick']) is not int or type(latch['delay_ticks']) is not int
                or not 1<=latch['delay_ticks']<=1000000 or not 0<=latch['armed_tick']<=now
                or latch['due_tick']!=latch['armed_tick']+latch['delay_ticks']
                or not isinstance(latch['route'],dict)
                or {k:v for k,v in latch['route'].items() if k!='handle'}!=dict(cpu=0,line=88,
                    vector=0x13372e8,priority=13,trigger_mode=1,source_type=0)
                or type(latch['route'].get('handle')) is not int or latch['route']['handle']<=0
                or type(latch['handshakes_started']) is not int or latch['handshakes_started']<0
                or type(latch['handshakes_completed']) is not int or latch['handshakes_completed']<0
                or not isinstance(events,list) or len(events)>len(self.CANCELLED_RETIREMENT)+CANCELLED_ENABLE_POLL_LIMIT):
            raise UnsupportedDevice('FE80 cancelled completion retained fields are outside the bounded profile')
        if hashlib.sha256(self.board.read_memory(0x5ff235d4,88,cpu_id=0)).hexdigest()!=latch['output_sha256']:
            raise UnsupportedDevice('FE80 discarded output bytes changed; numerical content remains unqualified')
        asserted=cancel['asserted_tick'];ready=cancel['enable_ready_tick'];enable_due=cancel['enable_due_tick']
        ready_polls=cancel.get('enable_ready_poll_count')
        if (ready is None and ready_polls is not None
                or ready is not None and (type(ready_polls) is not int
                    or not 0<=ready_polls<=len(events)-len(self.CANCELLED_RETIREMENT))):
            raise UnsupportedDevice('FE80 cancelled enable acknowledgment lost its retained poll boundary')
        if asserted is None:
            if events or cancel['state']!='armed' or ready is not None or enable_due is not None:
                raise UnsupportedDevice('FE80 cancelled completion has effects before its external event')
            if (self.phase!='routed' or self.busy
                    or self.handshakes_started!=latch['handshakes_started']
                    or self.handshakes_completed!=latch['handshakes_completed']):
                raise UnsupportedDevice('FE80 cancelled completion changed idle setup ownership')
            if self._cancelled_flags()!=bytes.fromhex('020000000001') or self._cancelled_route()!=latch['route']:
                raise UnsupportedDevice('FE80 cancelled completion consumer/route changed before delivery')
            return
        if type(asserted) is not int or not latch['due_tick']<=asserted<=now:
            raise UnsupportedDevice('FE80 cancelled completion event chronology changed')
        projection[0xfe80d804]=0
        previous=asserted
        for index,item in enumerate(events):
            if (not isinstance(item,dict) or set(item)!= {'operation','address','value','tick','cpu'}
                    or type(item['tick']) is not int or not previous<=item['tick']<=now or item['cpu']!=0):
                raise UnsupportedDevice('FE80 cancelled completion control event changed')
            previous=item['tick']
            if index<len(self.CANCELLED_RETIREMENT):
                expected=self.CANCELLED_RETIREMENT[index]
                if (item['operation'],item['address'],item['value'])!=expected:
                    raise UnsupportedDevice('FE80 cancelled completion differs from the whole stock retirement')
                if item['operation']=='write':projection[item['address']]=item['value']
            else:
                after_ready=ready is not None and index>=len(self.CANCELLED_RETIREMENT)+ready_polls
                if ((item['operation'],item['address'])!=('read',0xfe80d84c)
                        or item['value']!=int(after_ready)
                        or ready is not None and (item['tick']<ready if after_ready else item['tick']>ready)):
                    raise UnsupportedDevice('FE80 cancelled completion enable polling changed')
        count=len(events)
        if count<20:expected_state='irq-asserted' if not count else 'retired' if count==19 else 'retiring'
        elif count==20:expected_state='enable-requested'
        elif ready is None:expected_state='enable-pending'
        else:expected_state='complete' if count>21 and events[-1]['value']==1 else 'awaiting-enable-poll'
        if cancel['state']!=expected_state:
            raise UnsupportedDevice('FE80 cancelled completion state/control ledger differs')
        if count<21:
            if enable_due is not None or ready is not None:
                raise UnsupportedDevice('FE80 cancelled enable response precedes the actual request')
        else:
            if enable_due!=events[20]['tick']+self.handshake_ticks:
                raise UnsupportedDevice('FE80 cancelled enable delay changed')
            projection[0xfe80d84c]=0 if ready is None else 1
            if ready is not None and (type(ready) is not int or not enable_due<=ready<=now):
                raise UnsupportedDevice('FE80 cancelled enable response chronology changed')
        expected_phase='routed' if count<20 else 'enable-requested' if count==20 else 'enable-pending' if ready is None else 'enabled'
        if (self.phase!=expected_phase or self.busy!=(count>=21 and ready is None)
                or self.handshakes_started!=latch['handshakes_started']+(count>=21)
                or self.handshakes_completed!=latch['handshakes_completed']+(ready is not None)):
            raise UnsupportedDevice('FE80 cancelled enable hardware projection changed')

    def tick(self,board):
        if self.stream is not None:
            from . import fe80_stream
            fe80_stream.tick(self,board);return
        runtime=self._preview_runtime();cancel=runtime.get('cancelled_completion') if runtime else None
        if cancel is None:return
        state=cancel['state'];due=cancel['latch']['due_tick'] if state=='armed' else cancel['enable_due_tick'] if state=='enable-pending' else None
        if due is None or board.virtual_ticks<due:return
        self._validate_preview_runtime()
        if state=='armed':
            if self._cancelled_flags()!=bytes.fromhex('020000000001') or self._cancelled_route()!=cancel['latch']['route']:
                raise UnsupportedDevice('FE80 discarded completion no longer has its stock cancelled consumer')
            self.registers[0xfe80d804]=0
            cancel.update(state='irq-asserted',asserted_tick=board.virtual_ticks)
            board.interrupts.raise_irq(0,88)
            board.record('FE80 discarded-result no-error control completion IRQ',irq=88,
                limit='Output content unqualified and untouched; numerical callback must be suppressed by stock cancellation')
        else:
            self.registers[0xfe80d84c]=1;self.busy=False;self.phase='enabled'
            self.handshakes_completed+=1
            cancel.update(state='awaiting-enable-poll',enable_ready_tick=board.virtual_ticks,
                          enable_ready_poll_count=len(cancel['events'])-len(self.CANCELLED_RETIREMENT))

    def next_event_tick(self,now):
        if self.stream is not None:
            from . import fe80_stream
            return fe80_stream.due(self)
        runtime=self._preview_runtime();cancel=runtime.get('cancelled_completion') if runtime else None
        if cancel is None:return None
        if cancel['state']=='armed':return max(now,cancel['latch']['due_tick'])
        if cancel['state']=='enable-pending':return max(now,cancel['enable_due_tick'])
        return None

    def _cancelled_access(self,operation,core,address,value):
        runtime=self._preview_runtime();cancel=runtime['cancelled_completion'];index=len(cancel['events'])
        if core!=0 or cancel['state'] in ('armed','complete'):
            raise UnsupportedDevice('FE80 cancelled control access precedes IRQ or exceeds closed lifecycle')
        if index==0:
            bank=self.board.interrupts.banks[0]
            handle=int.from_bytes(self.board.read_memory(0xc6dbb4+0x27*4,4,cpu_id=0),'little')
            if (bank.active!=88 or bank.enabled&(1<<88) or handle!=0
                    or bank.config.get(0x400+4*88)!=0x42840
                    or self._cancelled_flags()!=bytes.fromhex('020000000001')):
                raise UnsupportedDevice('FE80 cancelled retirement is not owned by the actual unregistered IRQ88 handler')
        if index<len(self.CANCELLED_RETIREMENT):
            expected=self.CANCELLED_RETIREMENT[index]
            if (operation,address)!=expected[:2] or operation=='write' and value!=expected[2]:
                raise UnsupportedDevice('FE80 cancelled control access differs from the exact whole ISR transaction')
            result=expected[2]
            if index==19 and self._cancelled_flags()!=bytes.fromhex('030000000000'):
                raise UnsupportedDevice('FE80 cancelled enable requires the stock callback-suppressed cleanup branch')
            if operation=='write':self.registers[address]=result
            state='retired' if index==18 else 'enable-requested' if index==19 else 'enable-pending' if index==20 else 'retiring'
            if index==19:self.phase='enable-requested'
            elif index==20:
                self.phase='enable-pending';self.busy=True;self.registers[0xfe80d84c]=0
                self.handshakes_started+=1;cancel['enable_due_tick']=self.board.virtual_ticks+self.handshake_ticks
        elif ((operation,address)!=('read',0xfe80d84c)
                or index>=len(self.CANCELLED_RETIREMENT)+CANCELLED_ENABLE_POLL_LIMIT):
            raise UnsupportedDevice('FE80 cancelled enable polling exceeds the bounded physical control class')
        else:
            result=int(cancel['enable_ready_tick'] is not None);state='complete' if result else 'enable-pending'
        cancel['events'].append(dict(operation=operation,address=address,value=result,tick=self.board.virtual_ticks,cpu=core))
        cancel['state']=state
        return result

    def _preview_access(self,operation,core,address,value=None):
        runtime=self._preview_runtime()
        if runtime is None:return None
        self._validate_preview_runtime()
        index=len(runtime['events'])
        if index==len(self.PREVIEW_START):
            if 'cancelled_completion' in runtime:return self._cancelled_access(operation,core,address,value)
            raise UnsupportedDevice('FE80 preview awaits an unsupported opaque-program result; completion/retirement is not modeled')
        expected=self.PREVIEW_START[index]
        if (core!=0 or (operation,address)!=expected[:2]
                or operation=='write' and value!=expected[2]):
            raise UnsupportedDevice('FE80 preview access is outside the complete stock start transaction')
        result=expected[2]
        if operation=='write':self.registers[address]=0 if address==0xfe80c000 else value
        runtime['events'].append(dict(operation=operation,address=address,value=result,
            tick=self.board.virtual_ticks,cpu=core))
        runtime['state']='awaiting-result' if index+1==12 else 'starting'
        if index+1==12:self.board.record('FE80 stock preview start complete',
            limit='Request accepted; uploaded program remains opaque and no result has been generated')
        return result

    def _access(self,core,address,size):
        if not self.contains(address,size):
            raise UnsupportedDevice(f'Unknown FE80 controller register/access {address:#x}/{size}')
        if self.owner is not None and core!=self.owner:
            raise UnsupportedDevice('Cross-CPU FE80 setup transaction is unvalidated')

    def read(self,core,address,size):
        if self.stream is not None:
            from . import fe80_stream
            return fe80_stream.read(self,core,address)
        self._access(core,address,size)
        if self._preview_runtime() is not None:return self._preview_access('read',core,address)
        if address not in (0xfe80d84c,0xfe80c000):
            raise UnsupportedDevice(f'Unproved FE80 controller register read {address:#x}')
        return self.registers.get(address,0)

    def _handshake(self,core,kind):
        self.busy=True;self.registers[0xfe80d84c]=0
        self.handshakes_started+=1
        self.phase=kind+'-pending'
        self.board.record('FE80 exact setup handshake start',cpu=core,kind=kind,
            virtual_delay=self.handshake_ticks,limit='Bit0 acknowledgement only; no engine/output effect')
        def complete():
            self.registers[0xfe80d84c]=1;self.busy=False
            self.phase='reset-acknowledged' if kind=='reset' else 'enabled'
            self.handshakes_completed+=1
            self.board.record('FE80 exact setup handshake complete',cpu=core,kind=kind,status='0x1')
        self.board.schedule(self.handshake_ticks,complete)

    def _pointer(self,value):
        physical=value-0x40000000 if 0x40000000<=value<0x80000000 else value
        if physical not in self.PROGRAMS:
            raise UnsupportedDevice(f'Unknown FE80 program descriptor {value:#x}')
        return physical,self.PROGRAMS[physical]

    def _upload(self,core):
        pointer=self.registers[0xfe80c1c8];physical,known=self._pointer(pointer)
        descriptor=bytes(self.board.read_memory(pointer,16,cpu_id=core))
        if descriptor.hex()!=known['descriptor']:
            raise UnsupportedDevice('FE80 program descriptor bytes differ from unchanged stock table')
        if self.registers.get(0xfe80d848)!=((known['period']-1)<<16):
            raise UnsupportedDevice('FE80 program descriptor/period combination is unproved')
        data=bytes(self.board.read_memory(known['source']|0x40000000,known['bytes'],cpu_id=core))
        if len(data)!=known['bytes'] or hashlib.sha256(data).hexdigest()!=known['sha256']:
            raise UnsupportedDevice('FE80 program upload bytes differ from unchanged stock firmware')
        self.busy=True;self.upload_phase='pending';self.uploaded=False
        self.registers[0xfe80c000]=0;self.uploads_started+=1
        delay=max(self.handshake_ticks,(len(data)+self.upload_bytes_per_tick-1)//self.upload_bytes_per_tick)
        job=dict(cpu=core,descriptor_pointer=hex(pointer),descriptor_physical=hex(physical),
            descriptor_hex=descriptor.hex(),source=hex(known['source']),bytes=len(data),
            sha256=known['sha256'],prefix_hex=data[:256].hex(),latency_ticks=delay,
            completed=False,effect='Opaque internal program retained; no program execution/RAM/pixel/IRQ effect')
        self.last_upload=job
        self.board.record('FE80 exact program upload start',**job)
        def complete():
            self.program_data=data;self.registers[0xfe80c000]=0x10
            self.upload_phase='completed';self.uploaded=True;self.busy=False
            self.uploads_completed+=1;job['completed']=True
            self.board.record('FE80 exact program upload complete',**job)
        self.board.schedule(delay,complete)

    def write(self,core,address,size,value):
        if self.stream is not None:
            if not 0<=value<=0xffffffff:raise UnsupportedDevice('FE80 write exceeds a32-bit word')
            from . import fe80_stream
            fe80_stream.write(self,core,address,value);return
        self._access(core,address,size)
        if not 0<=value<=0xffffffff:
            raise UnsupportedDevice('FE80 write exceeds a32-bit word')
        if self._preview_runtime() is not None:
            self._preview_access('write',core,address,value);return
        if self.busy:
            raise UnsupportedDevice('FE80 setup mutation while acknowledgement is pending')
        if address==0xfe80d84c:
            raise UnsupportedDevice('FE80 handshake status is read-only')
        if address==0xfe80d840:
            if value!=0:raise UnsupportedDevice('Only stock FE80 reset-preparation value0 is supported')
            self.owner=core;self.phase='reset-prepared';self.upload_phase='idle';self.uploaded=False
            self.registers[0xfe80d84c]=0;self.route_ready=False;self.route_index=None
        elif address==0xfe80d844:
            if value==1 and self.phase=='reset-prepared':self._handshake(core,'reset')
            elif value==1 and self.phase=='enable-requested' and self.uploaded and self.route_ready:self._handshake(core,'enable')
            elif value==0 and self.phase=='period-programmed':self.phase='reset-released'
            else:raise UnsupportedDevice(f'Unknown FE80 handshake request/order phase={self.phase},value={value:#x}')
        elif address==0xfe80d848:
            if self.phase!='reset-acknowledged' or value not in self.PERIOD_VALUES:
                raise UnsupportedDevice('FE80 period/order is outside the selected stock caller values')
            self.phase='period-programmed'
        elif address==0xfe80d800:
            if value==0 and self.phase=='reset-released':self.phase='disabled'
            elif value==0x1ff and self.phase in self.UPLOAD_SETUP_PHASES and self.upload_phase=='released' and self.route_ready:self.phase='enable-requested'
            else:raise UnsupportedDevice(f'Unknown FE80 enable/order phase={self.phase},value={value:#x}')
        elif address==0xfe80cc20:
            if value!=0x44444 or self.phase!='disabled':raise UnsupportedDevice('Unknown FE80 routing value/order')
            self.phase='routed'
        elif address==0xfe80c540:
            if value==0x40001 and self.phase in self.UPLOAD_SETUP_PHASES and self.upload_phase=='idle':self.upload_phase='prepared'
            elif value==0x40000 and self.upload_phase=='release-set':self.upload_phase='released'
            else:raise UnsupportedDevice('Unknown FE80 program-channel control/order')
        elif address==0xfe80c1c8:
            if self.upload_phase!='prepared':raise UnsupportedDevice('FE80 program pointer before exact channel setup')
            _,known=self._pointer(value)
            if self.phase!=known.get('setup_phase','routed'):
                raise UnsupportedDevice('FE80 descriptor setup phase differs from its selected contract')
            self.upload_phase='pointer-set'
        elif address==0xfe80c1c0:
            if value==1 and self.upload_phase=='pointer-set':self._upload(core)
            elif value==0 and self.upload_phase=='acknowledged':self.upload_phase='stopped'
            elif value==0x100 and self.upload_phase=='stopped':self.upload_phase='release-set'
            else:raise UnsupportedDevice('Unknown FE80 program transfer command/order')
        elif address==0xfe80c000:
            if value!=0x10 or self.upload_phase!='completed' or self.registers.get(address)!=0x10:
                raise UnsupportedDevice('Only exact completed-program bit4 acknowledgement is supported')
            self.registers[address]=0;self.upload_phase='acknowledged'
            self.board.record('FE80 program completion acknowledged',cpu=core,value='0x10')
            return
        elif address==0xfe3b0000:
            if value!=7 or self.upload_phase!='released':raise UnsupportedDevice('Unknown companion route setup/order')
            self.route_index=0;self.route_ready=False
        elif address in (0xfe318040,0xfe318048,0xfe318010,0xfe318008):
            if (address==0xfe318040 and value==1 and self.route_ready and self.upload_phase=='released'):
                self.route_index=0;self.route_ready=False
            if self.route_index is None or self.route_index>=len(self.ROUTE_SEQUENCE) or (address,value)!=self.ROUTE_SEQUENCE[self.route_index]:
                raise UnsupportedDevice('Unknown companion reset/setup order or value')
            self.route_index+=1
            if self.route_index==len(self.ROUTE_SEQUENCE):
                self.route_ready=True
                self.board.record('FE80 companion exact setup complete',cpu=core,
                    limit='Register bookkeeping only; physical device/function/output unknown')
        else:raise UnsupportedDevice(f'Unknown FE80 write {address:#x}')
        self.registers[address]=value

    def snapshot(self):
        if self.stream is None:self._validate_preview_runtime()
        runtime=self._preview_runtime()
        return dict(device=type(self).__name__,phase=self.phase,upload_phase=self.upload_phase,
            busy=self.busy,owner=self.owner,uploaded=self.uploaded,route_ready=self.route_ready,
            handshakes_started=self.handshakes_started,handshakes_completed=self.handshakes_completed,
            uploads_started=self.uploads_started,uploads_completed=self.uploads_completed,
            last_upload=self.last_upload,
            preview_processing=None if runtime is None else dict(profile=runtime['latch']['profile'],
                state=runtime['state'],accepted_start_accesses=len(runtime['events']),
                input_sha256=runtime['latch']['input_sha256'],output_generated=False,
                cancelled_completion=None if 'cancelled_completion' not in runtime else
                    dict(profile=runtime['cancelled_completion']['latch']['profile'],
                        state=runtime['cancelled_completion']['state'],
                        accepted_control_accesses=len(runtime['cancelled_completion']['events']),
                        numerical_output_qualified=False)),
            registers={hex(k):hex(v) for k,v in sorted(self.registers.items())},
            limits=['Exact stock91E778/91E90C/91E88C/91E7EC setup plus explicit first AF preview91EE1C start; four known descriptor/blob hashes and period combinations.',
                    'Zero status reset, bit0 handshake and bit4 upload acknowledgement delays are explicit unmeasured model fixtures.',
                    'Uploaded programs are opaque retained data; no execution, numerical RAM output or pixels modeled.',
                    'Optional preview start retains acknowledged input identity and all12 real accesses; active numerical results remain unsupported.',
                    'A separately selected already-cancelled first job may deliver delayed no-error control IRQ88 and exact stock retirement/enable; discarded output contents remain unqualified and untouched.',
                    'Companion FE3B/FE31 roles are unverified; only the exact stock register setup order is retained.',
                    'Unknown commands, offsets, reads, descriptors, program mutations and concurrent transactions stop explicitly.'])
