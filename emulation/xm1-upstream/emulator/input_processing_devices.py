"""Bounded FFF10000 image-input/processing control, exact engine unverified.

Legacy X70 1.10 stock920E3C initialization and920E80/922024/92205C/923C78 control
clear operations are recovered. Global configuration setters/getters retain
their proven bit fields. The W0-bit0 -> bit0-set reset acknowledgment is
inferred from the actual92205C poll, with an explicit virtual delay. An
optional exact packed-command transmitter models its own bit0 busy clear;
it performs no DMA result write, image processing or external sensor. A
distinct opt-in source1 edge input can assert one actual IRQ29 per genuine
input0 enable generation; its event identity/delay/ack semantics are unmeasured.
A separate finite REC stream experiment preserves three startup edges then
permits at most three ACK-paced source0 edges within the same generation3.
It supplies no image data, DMA, software result or readiness callback.
X-M1 1.50 profiles admit guest-produced packed writes, upper-field-preserving
phase acknowledgments and independent source clocks with retained status flags.
Source periods and virtual-to-physical clock ratios are unmeasured.
"""
import hashlib
import json
from copy import copy
from enum import IntEnum
from dataclasses import dataclass

from . import strict_audit
from .hardware import UnsupportedDevice


class PhaseSource(IntEnum):
    PRIMARY = 0
    SECONDARY = 1

@dataclass
class PhaseClockEdge:
    cpu: int | None
    status_mask: int
    asserted_tick: int
    generation: int



XM1_CLOCK_PROFILE = 'xm1-150-configured-phase-clock-unmeasured'
COMMAND_PROFILE='stock-mode0-81-write-only'
PACKED_WRITES_PROFILE='xm1-150-packed-command-writes'
PROFILES=(COMMAND_PROFILE,PACKED_WRITES_PROFILE)
PHASE_PROFILE='stock-inactive-phase2-ack'
XM1_PHASE_PROFILE = 'xm1-150-phase2-ack-unmeasured'
PHASE_PROFILES=(PHASE_PROFILE,XM1_PHASE_PROFILE)
EVENT_PROFILE='stock-input0-single-external-edge-unmeasured'
DUAL_EVENT_PROFILE='stock-input0-source1-and-source0-single-edges-unmeasured'
REC_STARTUP_EVENT_PROFILE='stock-input0-three-startup-edges-unmeasured'
REC_STREAM_EVENT_PROFILE='stock-input0-three-startup-plus-three-source0-edges-unmeasured'
REC_STREAM_EXTRA_ATTEMPTS=3
REC_DISPLAY_EVENT_PROFILE='stock-input0-three-startup-plus-four-source0-edges-long-final-gap-unmeasured'
REC_DISPLAY_DUE_ROUTE_EVENT_PROFILE='declared-rec-display-input-edges-due-route-unmeasured'
MENU_CLOSE_EVENT_PROFILE='declared-rec-display-plus-one-close-source1-edge-unmeasured'
PREVIEW_REENTRY_EVENT_PROFILE='declared-rec-display-close-plus-one-reentry-source1-edge-unmeasured'
PREVIEW_READY_EVENT_PROFILE='declared-rec-display-close-reentry-plus-one-ready-source0-edge-unmeasured'
# Explicit host-selected continuation; never an automatic legacy-profile edge.
PREVIEW_FRAME_EVENT_PROFILE='declared-post-arm-preview-plus-one-source0-edge-unmeasured'
# Explicit host-selected continuous source clock (emulator/testbed/STREAM.md).
# Never part of any strict default; see enable_stream().
STREAM_EVENT_PROFILE='declared-testbed-continuous-sensor-stream-unmeasured'
CARD_HISTOGRAM_EVENT_PROFILE='declared-rec-display-preview-ready-card-late-histogram-epoch-unmeasured'
CARD_STAGING_EVENT_PROFILE='declared-rec-display-card-late-source-staging-unmeasured'
# Both explicit card profiles allow the data owners to project ordinal5.
# Only CARD_STAGING additionally supplies one later source-selection edge.
CARD_HISTOGRAM_EVENT_PROFILES=(CARD_HISTOGRAM_EVENT_PROFILE,CARD_STAGING_EVENT_PROFILE)
PREVIEW_READY_EVENT_PROFILES=(PREVIEW_READY_EVENT_PROFILE,*CARD_HISTOGRAM_EVENT_PROFILES)
PREVIEW_REENTRY_EVENT_PROFILES=(PREVIEW_REENTRY_EVENT_PROFILE,*PREVIEW_READY_EVENT_PROFILES)
MENU_CLOSE_EVENT_PROFILES=(MENU_CLOSE_EVENT_PROFILE,*PREVIEW_REENTRY_EVENT_PROFILES)
REC_DISPLAY_EVENT_PROFILES=(REC_DISPLAY_EVENT_PROFILE,REC_DISPLAY_DUE_ROUTE_EVENT_PROFILE,
                            *MENU_CLOSE_EVENT_PROFILES)
DUE_ROUTE_EVENT_PROFILES=(REC_DISPLAY_DUE_ROUTE_EVENT_PROFILE,*MENU_CLOSE_EVENT_PROFILES)
REC_DISPLAY_EXTRA_ATTEMPTS=4
REC_DISPLAY_FINAL_GAP_MULTIPLIER=8
REC_STREAM_EVENT_PROFILES=(REC_STREAM_EVENT_PROFILE,*REC_DISPLAY_EVENT_PROFILES)
EVENT_PROFILES=(EVENT_PROFILE,DUAL_EVENT_PROFILE,REC_STARTUP_EVENT_PROFILE,*REC_STREAM_EVENT_PROFILES,XM1_CLOCK_PROFILE)
# Exact packed tables made by13151FC; each allowed hash includes all five
# padding bytes per record. The original profile accepts only the reset hash;
# the separate packed-writes profile validates a bounded transmission grammar.
RESET_PACKET_SHA256='4e92bf6ce5295c7f035062613de0860dd68ecbaedb99e631d162231ff19a6556'
INITIAL_MODE_PACKET_SHA256='01d8f110810ba9c182b6c55941d964ade21e2fbc193c2ef8a6ec694958e48448'
COMMAND_PACKETS={
    RESET_PACKET_SHA256:dict(name='reset-table',records=256,
        source='Immutable145076C/1450F6C→1318980→13151FC'),
    INITIAL_MODE_PACKET_SHA256:dict(name='initial-mode27-table',records=90,
        source='Immutable1433CBC[27]→descriptor22→144F3BC; actual1316AE0 bit2 clear→1318C18→13151FC')}


class InputProcessingControl:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASE=0xfff10000
    RESET_PROFILE='stock-write0-bit0-reset-ack'
    # Fields written by recovered920EBC..921F98. Names beyond geometry are
    # intentionally absent until the hardware/driver enums are established.
    MASKS={0x04:0x00010003,0x08:0x03838183,0x0c:0x11110003,
           0x10:0x07170311,0x14:0x3fff1fff,0x18:0x3fff3fff,
           0x1c:0x3fff1fff,0x20:0x3fff3fff,0x24:0x3fff,
           0x28:0x3fff0fff,0x2c:0x3fff3fff,0x30:0x1183,
           0x34:0x3fff0fff,0x38:0x3fff3fff,0x3c:0x1fff,
           # Two indexed inactive subblocks: queried/cleared by120FF90.
           # Guest-written input enable latches; no stream/effect is generated.
           0x44:0,0x48:0x10001,0x4c:1,0x6c:0,0x70:1,0x74:1,0x94:0,0xac:0,
           # Indexed inactive geometry/modes92222C..9228A4. Positive
           # Enable-request strobes4C/74 are write-only1.
           0x50:0x00030007,0x78:0x00030007,
           0x54:0x3fff3fff,0x58:0xbfff3fff,0x5c:0xf,
           0x7c:0x3fff3fff,0x80:0xbfff3fff,0x84:0xf,
           0x60:0x3fff3fff,0x64:0x3fff3fff,0x68:0xf,
           0x88:0x3fff3fff,0x8c:0x3fff3fff,0x90:0xf,
           # Two indexed command configuration pairs9229F8/922C24.
           0x98:0x01030007,0x9c:0x01ff0007,
           0xb0:0x01030007,0xb4:0x01ff0007,
           # X-M1 00661D70/00661E78 indexed payload/enable words.
           0xa0:0xffffffff,0xa4:0xffff,0xa8:1,
           0xb8:0xffffffff,0xbc:0xffff,0xc0:1,
           # Channel0/1 passive DMA descriptor fields9236F8/92389C.
           # Arm, status and transfer effects require the narrow profile.
           0x140:0xff,0x24c:0xff,
           0x170:0x70703000,0x270:0x70703000,
           0x174:0x3ffffffc,0x178:0x3ffffffc,
           0x274:0x3ffffffc,0x278:0x3ffffffc,
           0x15c:7,0x25c:7,
           0x150:0,0x250:0,
           # RAW channel2/3: whole9239D4/923B60 descriptor setters/getters,
           # 920ADC IRQ selection and920A3C write-only acknowledgment.
           # IRQ enables retain configuration; they do not start a source,
           # perform DMA or fabricate a completion.
           0x350:7,0x36c:7,
           0x354:0x707000ff,0x370:0x707000ff,
           0x358:0x3ffffff0,0x374:0x3ffffff0,
           0x35c:1,0x378:1,
           0x360:0x7ffffff0,0x37c:0x7ffffff0,
           # Six indexed passive phase configs922F6C/923260 and shared923428.
           **{0xc4+index*0x14:0x8001c703 for index in range(6)},
           **{0xc8+index*0x14+word*4:0x3fff3fff for index in range(6) for word in range(4)},
           0x13c:0x10103,
           # Stock924420 event acknowledgment and924460/92454C IRQ selection.
           0x388:1,0x38c:0x10203}
    CONTROL_CLEAR_MASK=0x40004003
    COMMAND_CONFIG={0x98:2,0x140:0,0x170:0x00101000,0x178:0,0x15c:7}
    EVENT_IRQ=0x29
    raw_stream=None  # Opt-in pdaf_stream.RawChannelStream (class default keeps old checkpoints loadable).
    stream=None  # Class default so pre-stream checkpoints restore (see enable_stream).

    def __init__(self,board,*,reset_ticks=2,command_profile=None,command_ticks=2,
                 phase_profile=None,phase_ticks=2,event_profile=None,event_ticks=400000):
        if not isinstance(reset_ticks,int) or reset_ticks<2:
            raise ValueError('Input control reset delay must be at least2 virtual ticks')
        if command_profile is not None and command_profile not in PROFILES:
            raise ValueError('Unknown input command profile')
        if isinstance(command_ticks,bool) or not isinstance(command_ticks,int) or command_ticks<2:
            raise ValueError('Input command delay must be at least2 virtual ticks')
        if phase_profile is not None and phase_profile not in PHASE_PROFILES:
            raise ValueError('Unknown input phase command profile')
        if isinstance(phase_ticks,bool) or not isinstance(phase_ticks,int) or phase_ticks<2:
            raise ValueError('Input phase delay must be at least2 virtual ticks')
        if event_profile is not None and event_profile not in EVENT_PROFILES:
            raise ValueError('Unknown input external event profile')
        if isinstance(event_ticks,bool) or not isinstance(event_ticks,int) or event_ticks<1:
            raise ValueError('Input external event delay must be positive virtual ticks')
        self.board=board;self.reset_ticks=reset_ticks
        self.registers={offset:0 for offset in (0,0x384,*self.MASKS)}
        self.busy=False;self.reset_due=None;self.reset_requests=0;self.reset_completions=0
        self.writes=[];self.last_rejected=None
        self.command_profile=command_profile;self.command_ticks=command_ticks
        self.command_armed=None;self.pending_command=None;self.last_command=None
        self.dma_busy=False
        self.command_requests=0;self.command_completions=0
        self.command_cancellations = 0
        self.phase_clock_pending = {}
        self.phase_clock_due = {}
        self.phase_profile=phase_profile;self.phase_ticks=phase_ticks
        self.phase_busy=False;self.pending_phases={};self.last_phases={}
        self.phase_requests=0;self.phase_completions=0
        self.event_profile=event_profile;self.event_ticks=event_ticks
        self.event_enable_generation=0;self.event_attempted_generation=0
        self.event_due=None;self.event_target=None;self.event_pending=False
        self.event_source=None;self.event_irq=None;self.event_ack_selector=None
        self.event_arms=0;self.event_assertions=0;self.event_acknowledgments=0
        self.event_cancellations=0;self.last_event=None
        # Plain, bounded state for the separate finite stream profile only.
        # No timer closure, callback/task predicate or captured result is used.
        self.event_startup_acked={1:False,2:False,3:False}
        self.event_extra_attempts=0;self.event_ordinal=0
        self.event_followup=None;self.event_stream_blocked=False
        self.event_stream_cancellations=0;self.event_history=[]
        board.record('Input processing control fixture',base=hex(self.BASE),
            reset_profile=self.RESET_PROFILE,reset_ticks=reset_ticks,
            provenance='Stock920E3C init,92205C clear-bit0/poll-bit0 and global920EBC..921F98 setters/getters',
            limits='Initial registers are unmeasured zero inputs; reset response/delay inferred; engine purpose/physical timing unknown; no images/DMA/IRQs')
        if command_profile is not None:
            board.record('Input command transmission fixture',profile=command_profile,
                command_ticks=command_ticks,packet_catalog=COMMAND_PACKETS,
                scope=('Exact256-record reset-packet hash only' if command_profile==COMMAND_PROFILE
                       else 'Mode0/unit3/channel0,1..512 opcode81 eight-byte records with five zero padding bytes'),
                provenance='1210608 mode0→120FEBC channel0,13151FC eight-byte packing, hash-validated stock table packets',
                inference='Virtual write-only transmitter clears94 bit0 after bounded delay; external receiver/physical completion are not measured')
        if phase_profile is not None:
            board.record('Input inactive phase acknowledgment fixture',profile=phase_profile,
                phase_ticks=phase_ticks,provenance='131D248→1211028 mode2/wait1→922EB4 writes3/2→923260 polls3',
                inference='Delayed low-mode2→3 only before input enable, with shared13C=1; no image/IRQ/receiver operation is inferred')
        if event_profile is not None:
            board.record('Input external edge fixture',profile=event_profile,event_ticks=event_ticks,
                physical_irqs=['0x29'] if event_profile==EVENT_PROFILE else ['0x29','0x27'],
                provenance=('Stock48 enable latch; source1 38C10002/IRQ29/9DF038 selector0; '
                            'dual profile source0 38C10003/IRQ27/9DF0B8 selector1; '
                            'finite REC startup experiment adds generation3 38C10203/source0'),
                inference=(f'Three startup edges plus at most {self._stream_extra_limit()} matching-ACK-paced generation3 source0 attempts; physical identity/timing unmeasured; no image/DMA/pixels'
                    if event_profile in REC_STREAM_EVENT_PROFILES else
                    'One unmeasured virtual external edge per input0 enable generation; physical event identity/delay/ack meaning unproved; no image/DMA/pixels'))
        if event_profile in REC_STREAM_EVENT_PROFILES:
            board.record('Finite input source0 stream experiment',profile=event_profile,
                startup_pairs=['generation1/mask10002/source1','generation2/mask10003/source0',
                               'generation3/mask10203/source0'],
                extra_attempt_limit=self._stream_extra_limit(),event_ticks=event_ticks,
                provenance='Stock source0 IRQ27/9DF0B8 matching388=1; captured4002→1325EB0 registers13254CC after the third startup ACK',
                inference=(f'At most {self._stream_extra_limit()} additional unmeasured edges in generation3; each delay starts at the previous matching real ACK. '
                    + ('The separately selected display profile final gap is 8 times event_ticks. ' if event_profile in REC_DISPLAY_EVENT_PROFILES else '')
                    + 'No physical sensor cadence or downstream image/work completion is established'))
        if event_profile in DUE_ROUTE_EVENT_PROFILES:
            board.record('Input edge due-route policy fixture',profile=event_profile,
                inference='Initial arm retains its unique CPU owner; temporary IRQ masks before due do not cancel the engine timer. The same unique enabled owner is required at due assertion; engine/source/configuration/generation loss still cancels immediately.')
        if event_profile in MENU_CLOSE_EVENT_PROFILES:
            board.record('Finite input close source1 experiment',profile=event_profile,
                generation=4,mask='0x10202',physical_irq='0x29',ack_selector=0,
                provenance='Actual drained REC close: new48 enable, source1 9DF2F0/12F612C subscription; IMGDC inner5 waits3F00 before1260768 posts3B44',
                inference=f'One additional unmeasured external edge after all {3+self._stream_extra_limit()} declared REC-display edges are acknowledged; no continuous cadence, image/DMA/result or guest callback is supplied')

        if event_profile in PREVIEW_REENTRY_EVENT_PROFILES:
            board.record('Finite input REC reentry source1 experiment',profile=event_profile,
                generation=5,mask='0x10002',physical_irq='0x29',ack_selector=0,
                provenance='Actual Brightness REC reentry: fresh48 enable; source1 9DF038/924420 selector0 and12F612C→3F00 stock subscriber',
                inference=f'One additional unmeasured external edge after {4+self._stream_extra_limit()} matching real ACKs, on the preceding close owner. Only the separate preview-ready profile declares generation6; no image/DMA/result or guest callback is supplied')
        if event_profile in PREVIEW_READY_EVENT_PROFILES:
            board.record('Finite input preview-ready source0 experiment',profile=event_profile,
                generation=6,mask='0x10003',physical_irq='0x27',ack_selector=1,
                provenance='Actual preview receiver wait: source0 subscribers11F6008/priority2 and12F8704/priority1; whole9DF0B8 calls them before924420 selector1 ACK',
                inference=f'One unmeasured external edge after {5+self._stream_extra_limit()} matching real ACKs, on the reentry owner. No later enable, source0 follow-up, image/DMA/payload or firmware callback substitution is supplied')

    @staticmethod
    def _phase_index(offset):
        if 0xc4<=offset<=0x128 and (offset-0xc4)%0x14==0:
            return (offset-0xc4)//0x14
        return None

    def contains(self,address,size):
        return self.BASE<=address<self.BASE+0x400

    def _reject(self,operation,core,address,size,value=None,reason='Unknown input processing operation'):
        self.last_rejected=dict(operation=operation,cpu=core,address=hex(address),size=size,
            value=None if value is None else hex(value) if isinstance(value,int) else repr(value),reason=reason)
        self.board.record('Input processing request rejected',**self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    def _offset(self,operation,core,address,size,value=None):
        offset=address-self.BASE
        profile_offset=self.command_profile is not None and offset in (0x154,0x15c,0x160)
        if size!=4 or (offset not in (0,0x384) and offset not in self.MASKS and not profile_offset):
            self._reject(operation,core,address,size,value)
        # Passive fields can be introduced only at their first access. This
        # preserves a branch-only method reload before an untouched frontier;
        # it never changes an already executed boot prefix.
        self.registers.setdefault(offset,0)
        return offset

    def read(self,core,address,size):
        if hasattr(self,'processing_activation') and 'data' in self.processing_activation:
            from . import processing_image_delivery
            if processing_image_delivery.contains(self,address,size):
                return processing_image_delivery.access(self,'read',core,address,size)
        if hasattr(self,'processing_activation'):
            from . import input_processing_activation
            if input_processing_activation.contains(self,address,size):
                return input_processing_activation.access(self,'read',core,address,size)
        if self.event_profile == XM1_CLOCK_PROFILE and address == self.BASE + 0x384 and size == 4:
            return self.registers.get(0x384, 0)
        if self.event_profile in REC_STREAM_EVENT_PROFILES:
            self._audit_rec_stream()
            if (address==self.BASE+0x384 and size==4
                    and (self.registers.get(0x384,0)!=0 or self.event_pending or self.event_due is not None)):
                self._reject('read',core,address,size,reason='Unmodeled input event status')
        offset=self._offset('read',core,address,size)
        if offset in (0x150,0x250):
            self._reject('read',core,address,size,reason='Unproven channel status read')
        if offset in (0x4c,0x74):
            self._reject('read',core,address,size,reason='Input enable-request strobe has no proven readable status')
        if offset==0x388:
            self._reject('read',core,address,size,reason='Input event acknowledgment has no proven readable status')
        if offset==0x384:
            if self.registers[offset]!=0 or self.event_pending or self.event_due is not None:
                self._reject('read',core,address,size,reason='Unmodeled input event status')
            return 0 # Explicit absent events outside pending opt-in edge.
        if offset in (0x15c,0x25c):
            self._reject('read',core,address,size,reason='Command event acknowledgement is write-only in this profile')
        if offset in (0x350,0x36c):
            self._reject('read',core,address,size,reason='RAW channel acknowledgment has no proven readable status')
        if offset==0x160:
            if self.pending_command is not None or self.registers.get(0x154,0)!=0 or self.last_command is None:
                self._reject('read',core,address,size,reason='Unproven active DMA channel status')
            return 0 # Quiescent chosen transmitter after actual disable; not a DMA result.
        return self.registers[offset]

    def _arm_command(self,core,address,size,value):
        if value==0:
            if self.pending_command is not None:
                self._reject('write',core,address,size,value,reason='Stopping an active command transfer is unsupported')
            self.command_armed=None
            return
        if value!=0x10001 or self.command_armed is not None or self.pending_command is not None:
            self._reject('write',core,address,size,value,reason='Unsupported command channel arm')
        if any(self.registers.get(offset,0)!=wanted for offset,wanted in self.COMMAND_CONFIG.items()):
            self._reject('write',core,address,size,value,reason='Only exact X-M1 mode2/unit3 command configuration is supported')
        count_word=self.registers.get(0x9c,0)
        records=(count_word>>16)+1
        if (count_word&0xffff!=2 or not 1<=records<=512
                or self.command_profile==COMMAND_PROFILE and records!=256):
            self._reject('write',core,address,size,value,reason='Unsupported command count/unit for selected profile')
        packed_bytes=records*8
        source=self.registers.get(0x174,0)
        try:
            packed=self.board.read_memory(source,packed_bytes,cpu_id=core)
        except (ValueError,RuntimeError) as error:
            self._reject('write',core,address,size,value,reason=f'Command source is unavailable ({type(error).__name__})')
        packed=bytes(packed)
        digest=hashlib.sha256(packed).hexdigest()
        packet=COMMAND_PACKETS.get(digest)
        if len(packed)!=packed_bytes:
            self._reject('write',core,address,size,value,reason='Truncated packed command source')
        if self.command_profile==COMMAND_PROFILE and digest!=RESET_PACKET_SHA256:
            self._reject('write',core,address,size,value,reason='Unsupported packed command payload')
        if self.command_profile==PACKED_WRITES_PROFILE and any(
                packed[i]!=0x81 or packed[i+3:i+8]!=bytes(5) for i in range(0,packed_bytes,8)):
            self._reject('write',core,address,size,value,reason='Unsupported packed write opcode or padding')
        if packet is None:
            packet=dict(name='packed-write-records',records=records,
                source='X-M1 guest-produced packed opcode81 records; external table origin is unvalidated')
        wire=b''.join(packed[i:i+3] for i in range(0,packed_bytes,8))
        self.command_armed=dict(cpu=core,source=hex(source),records=records,stride=8,wire_bytes=len(wire),
            packet_name=packet['name'],packet_provenance=packet['source'],count_word=count_word,
            packed_sha256=digest,packed_hex=packed.hex(),wire_sha256=hashlib.sha256(wire).hexdigest(),
            wire_hex=wire.hex(),profile=self.command_profile,
            inference='Eight-byte command records inferred from actual13151FC writer and channel0 setup; external transmission not executed')
        self.board.record('Input command channel armed',cpu=core,source=hex(source),
            records=records,packet_name=packet['name'],packed_sha256=digest,profile=self.command_profile)

    def _start_command(self,core,address,size,value):
        if self.command_profile is None or value!=1 or self.command_armed is None or self.pending_command is not None:
            self._reject('write',core,address,size,value,reason='Unsupported indexed command start')
        if self.registers.get(0x154)!=0x10001:
            self._reject('write',core,address,size,value,reason='Command DMA channel is not armed')
        if (any(self.registers.get(offset,0)!=wanted for offset,wanted in self.COMMAND_CONFIG.items())
                or self.registers.get(0x9c,0)!=self.command_armed['count_word']
                or self.registers.get(0x174,0)!=int(self.command_armed['source'],16)):
            self._reject('write',core,address,size,value,reason='Command configuration changed after arm')
        source=int(self.command_armed['source'],16)
        packed=bytes(self.board.read_memory(source,self.command_armed['records']*8,cpu_id=core))
        if hashlib.sha256(packed).hexdigest()!=self.command_armed['packed_sha256']:
            self._reject('write',core,address,size,value,reason='Command source changed after arm')
        self.pending_command=dict(self.command_armed,requested_tick=self.board.virtual_ticks,
            due_tick=self.board.virtual_ticks+self.command_ticks,state='pending')
        self.dma_busy=True
        self.command_requests+=1
        self.board.record('Input command transmission requested',cpu=core,profile=self.command_profile,
            due_tick=self.pending_command['due_tick'],packed_sha256=self.pending_command['packed_sha256'])

    def _write_phase(self,core,address,size,offset,value,old):
        if value&~self.MASKS[offset]:
            self._reject('write',core,address,size,value,reason='Unknown indexed phase control fields')
        if offset in self.pending_phases:
            self._reject('write',core,address,size,value,reason='Changing a pending phase request is unsupported')
        if self.phase_busy and value not in (2,3):
            self._reject('write',core,address,size,value,reason='Only other exact inactive phase staging is supported while pending')
        if value&3!=2:return
        if hasattr(self,'still_return_phases'):
            from . import input_still_return
            if input_still_return.request(self,core,address,size,offset,value,old):return
        inactive_required = self.phase_profile == PHASE_PROFILE
        transition = (old == 3 and value == 2) if inactive_required else ((old & 3) == 3 and (value & 3) == 2 and (old & ~3) == (value & ~3))
        if (self.phase_profile not in PHASE_PROFILES or not transition
                or self.registers.get(0x13c, 0) != 1 or self.registers.get(0x70, 0) & 1
                or inactive_required and self.registers.get(0x48, 0) & 1):
            self._reject('write', core, address, size, value,
                         reason='Phase acknowledgment requires staged mode3, mode2 request and shared enable')
        self.pending_phases[offset]=dict(cpu=core,index=self._phase_index(offset),
            requested_tick=self.board.virtual_ticks,due_tick=self.board.virtual_ticks+self.phase_ticks,
            request=hex(value),profile=self.phase_profile)
        self.phase_busy=True;self.phase_requests+=1
        self.board.record('Input inactive phase acknowledgment requested',**self.pending_phases[offset])

    def write(self,core,address,size,value):
        if hasattr(self,'processing_activation') and 'data' in self.processing_activation:
            from . import processing_image_delivery
            if processing_image_delivery.contains(self,address,size):
                return processing_image_delivery.access(self,'write',core,address,size,value)
        if hasattr(self,'processing_activation'):
            from . import input_processing_activation
            if input_processing_activation.contains(self,address,size):
                return input_processing_activation.access(self,'write',core,address,size,value)
        if self.event_profile in REC_STREAM_EVENT_PROFILES:
            self._validate_rec_stream()
            if type(value) is not int or not 0<=value<=0xffffffff:
                self._reject('write',core,address,size,value,reason='Finite input stream requires typed unsigned32-bit MMIO words')
        value&=0xffffffff
        if self.event_profile in REC_STREAM_EVENT_PROFILES and address==self.BASE+0x384 and size==4:
            self._reject('write',core,address,size,value,reason='Input status register is read-only')
        offset=self._offset('write',core,address,size,value)
        preview=self._preview_frame_record()
        if (preview is not None and preview['state']=='asserted' and offset==0x388
                and value==1 and (type(core) is not int or core!=preview['cpu'])):
            self._reject('write',core,address,size,value,reason='Preview frame ACK owner changed')
        old=self.registers[offset]
        if offset == 0x94 and value == 0 and self.pending_command is not None:
            self.last_command = dict(self.pending_command, state='canceled', canceled_tick=self.board.virtual_ticks)
            self.pending_command = self.command_armed = None
            self.dma_busy = False
            self.command_cancellations += 1
            self.board.record('Input command transmitter stopped by guest',
                              profile=self.command_profile, packed_sha256=self.last_command['packed_sha256'])
        if self.pending_command is not None and offset != 0x388:
            self._reject('write',core,address,size,value,reason='Changing input configuration during command transfer is unsupported')
        if self.command_armed is not None and offset not in (0x94,0x154,0x15c,0x388):
            self._reject('write',core,address,size,value,reason='Changing armed command configuration is unsupported')
        if self.phase_busy and self._phase_index(offset) is None and offset != 0x388:
            self._reject('write',core,address,size,value,reason='Changing input configuration during phase acknowledgment is unsupported')
        if self._phase_index(offset) is not None:
            self._write_phase(core,address,size,offset,value,old)
        elif offset in (0x4c,0x74):
            if value!=1:self._reject('write',core,address,size,value,reason='Unsupported input enable-request strobe')
        elif offset==0x154:
            self._arm_command(core,address,size,value)
        elif offset==0x15c:
            if value&~7:
                self._reject('write',core,address,size,value,reason='Unsupported command event acknowledgement mask')
        elif offset in (0x350,0x36c):
            if value&~7:
                self._reject('write',core,address,size,value,reason='Unsupported RAW channel acknowledgment mask')
        elif offset==0x94 and value:
            self._start_command(core,address,size,value)
        elif offset in (0x160,0x384):
            self._reject('write',core,address,size,value,reason='Input status register is read-only')
        elif offset==0:
            # All recovered global writers only clear these fields. Firmware
            # never writes a positive ready bit; only this reset contract does.
            if value&~old or (old^value)&~self.CONTROL_CLEAR_MASK:
                self._reject('write',core,address,size,value,reason='Unproven global control command')
        else:
            valid=not value&~self.MASKS[offset]
            if offset in (0x08,0x0c,0x30):valid=valid and value&3!=3
            if offset==0x0c and value&3:valid=valid and not value&0x01010000
            if offset==0x10:
                valid=valid and (value>>24)&7!=7 and (value>>16)&7!=7 and (value>>8)&3!=3
            if offset==0x24:valid=valid and value<=0x3000
            if offset in (0x170,0x270):valid=valid and (value>>20)&7<=4
            if offset in (0x354,0x370):valid=valid and (value>>20)&7<=4
            if offset in (0x50,0x78):valid=valid and (value>>16)&3!=3 and (value>>1)&3!=3
            if offset==0x38c and value&0x10000:valid=valid and bool(value&0x202)
            if not valid:
                self._reject('write',core,address,size,value,reason='Unknown input processing configuration field or channel start')
        if getattr(self,'raw_readout',None) is not None:
            from . import afs_still_readout
            afs_still_readout.observe_write(self,core,offset,value)
        self.registers[offset]=value
        item=dict(cpu=core,address=hex(address),value=hex(value),old=hex(old))
        self.writes.append(item)
        if offset in (0x350,0x36c):
            # No RAW event exists in this passive contract. W1 ACK masks
            # must never become a readable ready/result value.
            self.registers[offset]=0
            raw_stream=getattr(self,'raw_stream',None)
            if raw_stream is not None:raw_stream.on_ack(2 if offset==0x350 else 3,value)
            self.board.record('RAW input channel acknowledgment written by guest',**item,
                channel=2 if offset==0x350 else 3,
                limits='Write-only acknowledgment; no RAW source, DMA, completion, callback or IRQ is supplied')
        if offset in (0x4c,0x48,0x74,0x70):
            if offset==0x48 and value&1 and not old&1:
                self.event_enable_generation+=1
            self.board.record('Input enable latch written by guest',**item,
                limits='Guest-written retention; optional independent external edge input; no image, automatic enable or pixels')
        if offset in (0x388,0x38c):
            self.board.record('Input event acknowledgment or selection written by guest',**item,
                limits='Raw selector/mask retained; optional independent external edge; status384 bits remain unresolved')
            if offset == 0x388 and self.event_profile == XM1_CLOCK_PROFILE:
                self._ack_xm1_clock(value)
            elif offset==0x388 and self._stream_ack(value):
                pass
            elif offset==0x388 and value==self.event_ack_selector and self.event_pending:
                self.event_pending=False
                self.event_acknowledgments+=1
                self.last_event=dict(self.last_event,acknowledged_tick=self.board.virtual_ticks,acknowledge_selector=value)
                if self.event_profile in REC_STREAM_EVENT_PROFILES:
                    self._ack_rec_stream()
                self.board.record(f'Input source{self.event_source} external edge acknowledged (inferred)',**self.last_event,
                    inference='Only matching real ISR selector clears the virtual source flag; controller pending/IRQ return remain kernel-owned')
        if offset==0 and not value&1:
            self.busy=True;self.reset_due=self.board.virtual_ticks+self.reset_ticks
            self.reset_requests+=1
            self.board.record('Input processing reset requested',**item,due_tick=self.reset_due,
                profile=self.RESET_PROFILE)
        if (self.event_profile != XM1_CLOCK_PROFILE and self.event_due is not None and (not self._event_gates()
                or self._event_identity()!=(self.event_source,self.event_irq,self.event_ack_selector)
                or self.event_enable_generation!=self.event_attempted_generation)):
            self._cancel_event(self.board,'Guest enable/config/idle gates changed before edge')
        if (self.event_profile in REC_STREAM_EVENT_PROFILES and self.event_attempted_generation==3
                and (not self._event_gates() or self._event_identity()!=(0,0x27,1))):
            self._block_rec_stream('Guest enable/config/idle gates broke the generation3 continuation')
        if preview is not None:
            if preview['state']=='waiting' and not self._preview_frame_gates(preview):
                self._cancel_preview_frame('Guest input engine/configuration changed before the post-arm edge')
            elif preview['state']=='asserted' and offset==0x388 and value==1:
                preview.update(state='acknowledged',acknowledged_tick=self.board.virtual_ticks)
                self.board.record('Post-arm preview input acknowledged',**preview,
                    effects='Real selector1 ACK; original ten-event ledger and controller pending remain unchanged')

    @staticmethod
    def _preview_digest(value):
        return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()

    def _preview_frame_record(self):
        token=self.event_followup
        return token if type(token) is dict and token.get('profile')==PREVIEW_FRAME_EVENT_PROFILE else None

    def _preview_frame_prefix(self):
        # Validate the legacy ledger independently. Its records/counters/source
        # flags are retained unchanged by this separately selected experiment.
        view=copy(self);view.event_followup=None;view._validate_rec_stream()
        if (self.event_profile!=PREVIEW_READY_EVENT_PROFILE
                or (self.event_arms,self.event_assertions,self.event_acknowledgments)!=(10,10,10)
                or len(self.event_history)!=10 or any(e['state']!='acknowledged' for e in self.event_history)
                or (self.last_event['generation'],self.last_event['ordinal'],self.last_event['attempt'],
                    self.last_event['cpu'])!=(6,1,10,0)):
            raise UnsupportedDevice('Post-arm preview edge requires the exact ten-ACK generation6 prefix')

    def _preview_producer_anchor(self,validate_live=False):
        devices=[d for d in self.board.devices if type(d).__name__=='ProducerControl']
        if len(devices)!=1:raise UnsupportedDevice('Post-arm preview edge requires one producer')
        device=devices[0]
        if device.board is not self.board:
            raise UnsupportedDevice('Post-arm preview producer belongs to a different board')
        record=device.preview_configuration
        if type(record) is not dict or type(record.get('request')) is not dict:
            raise UnsupportedDevice('Post-arm preview edge requires the new producer request')
        request=record['request']
        if (type(request.get('step')) is not int or request['step']!=22
                or request.get('state')!='enabled-awaiting-fresh-input'
                or type(request.get('cpu')) is not int or request['cpu']!=0
                or type(request.get('enabled_tick')) is not int
                or not self.last_event['acknowledged_tick']<request['enabled_tick']<=self.board.virtual_ticks):
            raise UnsupportedDevice('Post-arm preview edge requires the completed new owner/enable request')
        if validate_live:
            device._validate_preview_request(record)
        # Later real callbacks may extend their own independent data/query
        # ledgers. Bind only the immutable observed22-access request fields.
        fields=('cpu','step','state','accesses','prior_words','current_words','history_sha256',
                'started_tick','armed_tick','enabled_tick','route',
                'configuration_sha256','retained_configuration_sha256')
        if not set(fields)<=set(request):
            raise UnsupportedDevice('Post-arm preview producer request is incomplete')
        return dict(enabled_tick=request['enabled_tick'],sha256=self._preview_digest({k:request[k] for k in fields}))

    def _preview_frame_gates(self,record):
        return (self.event_enable_generation==6 and self.registers.get(0x48)==1
            and self.registers.get(0x4c)==1 and self.registers.get(0x70)==0
            and self.registers.get(0x38c)==0x10003 and self.busy is False and self.phase_busy is False
            and self.pending_command is None and self.command_armed is None
            and all(type(v) is int for k,v in self.registers.items() if k!=0x388)
            and {k:v for k,v in self.registers.items() if k!=0x388}==record['register_words'])

    def _preview_frame_route(self):
        banks=self.board.interrupts.banks
        if (len(banks)!=2 or any(type(bank.enabled) is not int or not 0<=bank.enabled<1<<128 for bank in banks)):
            raise UnsupportedDevice('Post-arm preview IRQ enable banks changed type/domain')
        if self._event_route(39)!=0:return False
        bank=banks[0]
        if (type(bank.config) is not dict or type(bank.source_config) is not int
                or not 0<=bank.source_config<1<<128 or type(bank.pending) is not int
                or not 0<=bank.pending<1<<128):
            raise UnsupportedDevice('Post-arm preview IRQ registration changed type/domain')
        return (bank.config.get(0x400+4*39)==0x9df0b8
            and bank.config.get(0x800+4*39)==10 and (bank.source_config>>39)&1==1
            and not bank.pending&(1<<39) and bank.active!=39 and bank.injected!=39)

    def arm_preview_frame(self,profile):
        """Declare one future clock edge after the actual new producer arm.

        Scene ordinal4 is a virtual convention. No input payload, status bit,
        RAM write, software callback or result is supplied by this API.
        """
        if profile!=PREVIEW_FRAME_EVENT_PROFILE or self.event_followup is not None:
            raise UnsupportedDevice('Unknown or repeated post-arm preview input experiment')
        self._preview_frame_prefix();anchor=self._preview_producer_anchor(True)
        now=self.board.virtual_ticks
        if type(now) is not int or not 0<=now<1<<64 or type(self.event_ticks) is not int or not 1<=self.event_ticks<(1<<64)-now:
            raise UnsupportedDevice('Post-arm preview edge requires bounded typed delay')
        words={k:v for k,v in self.registers.items() if k!=0x388}
        record=dict(profile=profile,base_profile=self.event_profile,generation=6,ordinal=2,attempt=11,
            frame=4,cpu=0,source=0,physical_irq='0x27',ack_selector=1,state='waiting',
            requested_tick=now,armed_tick=now,due_tick=now+self.event_ticks,delay_ticks=self.event_ticks,
            asserted_tick=None,acknowledged_tick=None,canceled_tick=None,reason=None,
            history_sha256=self._preview_digest(self.event_history),producer=anchor,register_words=words)
        if not self._preview_frame_gates(record) or not self._preview_frame_route():
            raise UnsupportedDevice('Post-arm preview edge requires the live idle source0 owner/route')
        self.event_followup=record
        self.board.record('Post-arm preview input explicitly armed',**record,
            limits='One external source0 edge only; original ten events retained; no image/result/guest callback')
        return dict(record)

    def _validate_preview_frame(self):
        record=self._preview_frame_record()
        if record is None:raise UnsupportedDevice('Post-arm preview marker profile changed')
        self._preview_frame_prefix()
        expected={'profile','base_profile','generation','ordinal','attempt','frame','cpu','source',
            'physical_irq','ack_selector','state','requested_tick','armed_tick','due_tick','delay_ticks',
            'asserted_tick','acknowledged_tick','canceled_tick','reason','history_sha256','producer','register_words'}
        if (set(record)!=expected or type(self.board.virtual_ticks) is not int
                or not 0<=self.board.virtual_ticks<1<<64 or record.get('base_profile')!=self.event_profile
                or any(type(record.get(k)) is not int for k in ('generation','ordinal','attempt','frame','cpu','source','ack_selector'))
                or tuple(record[k] for k in ('generation','ordinal','attempt','frame','cpu','source','ack_selector'))!=(6,2,11,4,0,0,1)
                or record.get('physical_irq')!='0x27' or record.get('history_sha256')!=self._preview_digest(self.event_history)
                or record.get('producer')!=self._preview_producer_anchor()
                or type(record.get('register_words')) is not dict
                or set(record['register_words'])!=set(self.registers)-{0x388}
                or any(type(k) is not int or type(v) is not int or not 0<=v<1<<32 for k,v in record['register_words'].items())
                or any(type(record.get(k)) is not int or not 0<=record[k]<=self.board.virtual_ticks
                    for k in ('requested_tick','armed_tick'))
                or record['armed_tick']!=record['requested_tick']
                or record['requested_tick']<record['producer']['enabled_tick']
                or type(record.get('delay_ticks')) is not int or record['delay_ticks']!=self.event_ticks
                or type(record.get('due_tick')) is not int or not record['requested_tick']<record['due_tick']<1<<64
                or record['due_tick']!=record['requested_tick']+record['delay_ticks']
                or record.get('state') not in ('waiting','asserted','acknowledged','canceled')):
            raise UnsupportedDevice('Post-arm preview event identity/anchor/delay changed')
        asserted=record['asserted_tick'];ack=record['acknowledged_tick'];canceled=record['canceled_tick']
        if (record['state'] in ('asserted','acknowledged')):
            if type(asserted) is not int or not record['due_tick']<=asserted<=self.board.virtual_ticks:
                raise UnsupportedDevice('Post-arm preview assertion time changed')
        elif asserted is not None:raise UnsupportedDevice('Post-arm preview event claims an unobserved assertion')
        if (record['state']=='acknowledged' and (type(ack) is not int or not asserted<=ack<=self.board.virtual_ticks)
                or record['state']!='acknowledged' and ack is not None):
            raise UnsupportedDevice('Post-arm preview ACK time changed')
        if record['state']=='canceled':
            if type(canceled) is not int or not record['requested_tick']<=canceled<=self.board.virtual_ticks or type(record['reason']) is not str or not record['reason']:
                raise UnsupportedDevice('Post-arm preview cancellation changed')
        elif canceled is not None or record['reason'] is not None:
            raise UnsupportedDevice('Post-arm preview event claims an unobserved cancellation')
        return record

    def _cancel_preview_frame(self,reason):
        record=self._preview_frame_record()
        record.update(state='canceled',canceled_tick=self.board.virtual_ticks,reason=reason)
        self.board.record('Post-arm preview input canceled',**record,effects='No IRQ, replacement edge or result')

    def _tick_preview_frame(self,board):
        try:
            record=self._validate_preview_frame()
            if record['state']!='waiting':return
            if not self._preview_frame_gates(record):
                self._cancel_preview_frame('Input engine/source/configuration changed before the edge');return
            if board.virtual_ticks<record['due_tick']:return
            if not self._preview_frame_route():
                self._cancel_preview_frame('Unique exact source0 route unavailable at due edge');return
            self._preview_producer_anchor(True)
            record.update(state='asserted',asserted_tick=board.virtual_ticks)
            board.queue_irq(0,39)
            board.record('Post-arm preview input asserted',**record,
                effects='Physical IRQ39 only; real firmware owns subscribers/ACK; no payload/status/RAM')
        except UnsupportedDevice as error:
            board.failure=dict(message=str(error),cpu=0,device='InputProcessingControl',operation='preview-edge')

    def preview_frame_epoch(self):
        record=self._validate_preview_frame()
        if record['state'] not in ('asserted','acknowledged'):return None
        return {k:record[k] for k in ('profile','generation','ordinal','attempt','frame','cpu','source','physical_irq','asserted_tick')}

    def next_event_tick(self,now):
        due=[self.reset_due,self.event_due]
        if self.stream is not None:due.append(self.stream['next_due'])
        if hasattr(self,'processing_activation') and 'data' in self.processing_activation:
            from . import processing_image_delivery
            due.append(processing_image_delivery.next_event_tick(self,now))
        if getattr(self,'raw_readout',None) is not None:
            from . import afs_still_readout
            due.append(afs_still_readout.due(self))
        if self.event_profile in REC_STREAM_EVENT_PROFILES and self.event_followup is not None:
            preview=self._preview_frame_record()
            if preview is not None:
                if preview['state']=='waiting':due.append(preview['due_tick'])
            else:due.append(now) # Let the ordinary device tick attach the ACK-paced timer.
        if self.pending_command is not None:due.append(self.pending_command['due_tick'])
        due.extend(item['due_tick'] for item in self.pending_phases.values())
        due=[value for value in due if value is not None]
        return min(due) if due else None

    def _event_identity(self):
        mask=self.registers.get(0x38c,0)
        if (self.event_profile in PREVIEW_READY_EVENT_PROFILES
                and self.event_enable_generation==6 and mask==0x10003):
            return (0,0x27,1)
        if (self.event_profile in PREVIEW_REENTRY_EVENT_PROFILES
                and self.event_enable_generation==5 and mask==0x10002):
            return (1,0x29,0)
        if (self.event_profile in MENU_CLOSE_EVENT_PROFILES
                and self.event_enable_generation==4 and mask==0x10202):
            return (1,0x29,0)
        if self.event_profile in (REC_STARTUP_EVENT_PROFILE,*REC_STREAM_EVENT_PROFILES):
            return {(1,0x10002):(1,0x29,0),(2,0x10003):(0,0x27,1),
                    (3,0x10203):(0,0x27,1)}.get((self.event_enable_generation,mask))
        if self.event_profile in (EVENT_PROFILE,DUAL_EVENT_PROFILE) and mask==0x10002:
            return (1,0x29,0)
        if self.event_profile==DUAL_EVENT_PROFILE and mask==0x10003:
            return (0,0x27,1)
        return None

    def _event_gates(self):
        return (self.registers.get(0x48,0)&1 and not self.registers.get(0x70,0)&1
            and self._event_identity() is not None
            and not self.busy and self.pending_command is None and self.command_armed is None
            and not self.phase_busy)

    def _event_route(self,line):
        if not hasattr(self.board,'interrupts'):
            return None
        targets=[index for index,bank in enumerate(self.board.interrupts.banks)
                 if bank.enabled&(1<<line)]
        if len(targets)>1:
            raise UnsupportedDevice(f'Input external edge requires one enabled IRQ{line:#x} CPU route')
        return targets[0] if targets else None

    def _cancel_event(self,board,reason):
        self.event_cancellations+=1
        self.last_event=dict(self.last_event,canceled_tick=board.virtual_ticks,state='canceled',reason=reason)
        self.event_due=None;self.event_target=None
        if self.event_profile in REC_STREAM_EVENT_PROFILES:
            self._save_rec_stream_event()
            if self.event_attempted_generation==3:self._block_rec_stream(reason)
        board.record(f'Input source{self.event_source} external edge canceled',**self.last_event)

    def _save_rec_stream_event(self):
        self.event_history[-1]=dict(self.last_event)

    def _block_rec_stream(self,reason):
        """End this finite continuation, without clearing an asserted IRQ."""
        self.event_followup=None
        if not self.event_stream_blocked:
            self.event_stream_blocked=True
            self.event_stream_cancellations+=1
            self.board.record('Input generation3 finite continuation canceled',
                profile=self.event_profile,generation=self.event_enable_generation,
                ordinal=self.event_ordinal,extra_attempts=self.event_extra_attempts,
                canceled_tick=self.board.virtual_ticks,reason=reason,
                effects='No controller pending clear, replacement edge, data or completion')

    def _ack_rec_stream(self):
        event=self.last_event
        self.last_event=dict(event,state='acknowledged')
        self._save_rec_stream_event()
        if event['ordinal']==1 and event['generation']<=3:
            self.event_startup_acked[event['generation']]=True
        if (event['generation']==3 and not self.event_stream_blocked
                and all(self.event_startup_acked.values())
                and self.event_extra_attempts<self._stream_extra_limit()):
            if self._event_gates() and self._event_identity()==(0,0x27,1):
                self.event_followup=dict(generation=3,route=event['cpu'],
                    acknowledged_tick=self.board.virtual_ticks,after_attempt=event['attempt'])
            else:self._block_rec_stream('Matching ACK arrived outside the generation3 enable/mask/idle gates')

    def _stream_extra_limit(self):
        if self.event_profile==CARD_STAGING_EVENT_PROFILE:return REC_DISPLAY_EXTRA_ATTEMPTS+1
        return REC_DISPLAY_EXTRA_ATTEMPTS if self.event_profile in REC_DISPLAY_EVENT_PROFILES else REC_STREAM_EXTRA_ATTEMPTS

    def _stream_delay(self,ordinal):
        if type(ordinal) is not int or not 1<=ordinal<=1+self._stream_extra_limit():
            raise UnsupportedDevice('Finite input ordinal is outside the selected profile')
        late=(self.event_profile in REC_DISPLAY_EVENT_PROFILES and ordinal==1+REC_DISPLAY_EXTRA_ATTEMPTS
              or self.event_profile==CARD_STAGING_EVENT_PROFILE and ordinal==2+REC_DISPLAY_EXTRA_ATTEMPTS)
        return self.event_ticks*(REC_DISPLAY_FINAL_GAP_MULTIPLIER if late else 1)

    def _audit_rec_stream(self):
        """Validate the ledger when its counters or latest record changed.

        Writes always validate. Reads and ticks skip an unchanged ledger;
        X70_EMULATOR_AUDIT=1 restores the full audit on every access.
        """
        history=self.event_history
        key=(self.event_arms,self.event_assertions,self.event_acknowledgments,
             self.event_cancellations,self.event_extra_attempts,self.event_ordinal,
             self.event_enable_generation,self.event_attempted_generation,
             self.event_stream_cancellations,self.event_pending,self.event_stream_blocked,
             self.event_due,self.event_followup is None,
             len(history) if isinstance(history,list) else None,
             # Model updates to the latest record always change its state.
             history[-1].get('state')
                 if isinstance(history,list) and history and isinstance(history[-1],dict) else None)
        if strict_audit.needed(self,key):
            self._validate_rec_stream()
            strict_audit.passed(self,key)

    def _validate_rec_stream(self):
        """Check the finite attempt/ACK ledger before any new stimulus."""
        counts=(self.event_arms,self.event_assertions,self.event_acknowledgments,
                self.event_cancellations,self.event_extra_attempts,self.event_ordinal,
                self.event_enable_generation,self.event_attempted_generation,
                self.event_stream_cancellations)
        if (any(type(v) is not int or v<0 for v in counts)
                or type(self.event_ticks) is not int or self.event_ticks<1
                or not 0<=self.event_extra_attempts<=self._stream_extra_limit()
                or self.event_arms>3+self._stream_extra_limit()+int(self.event_profile in MENU_CLOSE_EVENT_PROFILES)
                    +int(self.event_profile in PREVIEW_REENTRY_EVENT_PROFILES)
                    +int(self.event_profile in PREVIEW_READY_EVENT_PROFILES)
                or not 0<=self.event_attempted_generation<=3+int(self.event_profile in MENU_CLOSE_EVENT_PROFILES)
                    +int(self.event_profile in PREVIEW_REENTRY_EVENT_PROFILES)
                    +int(self.event_profile in PREVIEW_READY_EVENT_PROFILES)
                or self.event_attempted_generation>self.event_enable_generation
                or type(self.event_pending) is not bool or type(self.event_stream_blocked) is not bool
                or self.event_stream_cancellations!=int(self.event_stream_blocked)
                or not isinstance(self.event_history,list) or len(self.event_history)!=self.event_arms
                or not isinstance(self.event_startup_acked,dict)
                or set(self.event_startup_acked)!={1,2,3}
                or any(type(k) is not int or type(v) is not bool
                       for k,v in self.event_startup_acked.items())):
            raise UnsupportedDevice('Finite input stream counters/history have unknown type or exceed their declared bounds')
        acked={1:False,2:False,3:False};extras=assertions=acks=canceled=0
        display_attempts=3+self._stream_extra_limit()
        previous_generation=0
        for index,event in enumerate(self.event_history):
            if not isinstance(event,dict):
                raise UnsupportedDevice('Finite input stream attempt record is not a dictionary')
            generation=event.get('generation');ordinal=event.get('ordinal')
            if (type(generation) is not int or generation not in
                    ((1,2,3,4,5,6) if self.event_profile in PREVIEW_READY_EVENT_PROFILES else
                     (1,2,3,4,5) if self.event_profile in PREVIEW_REENTRY_EVENT_PROFILES else
                     (1,2,3,4) if self.event_profile in MENU_CLOSE_EVENT_PROFILES else (1,2,3))):
                raise UnsupportedDevice('Finite input stream generation has unknown type or identity')
            identity={1:(1,0x29,0),2:(0,0x27,1),3:(0,0x27,1),4:(1,0x29,0),5:(1,0x29,0),6:(0,0x27,1)}[generation]
            expected_ordinal=2+extras if generation==previous_generation==3 else 1
            if (type(ordinal) is not int or ordinal!=expected_ordinal
                    or generation<previous_generation or generation==previous_generation and generation!=3
                    or type(event.get('attempt')) is not int or event['attempt']!=index+1
                    or event.get('profile')!=self.event_profile
                    or (event.get('source'),event.get('physical_irq'),event.get('ack_selector'))
                        !=(identity[0],hex(identity[1]),identity[2])
                    or type(event.get('source')) is not int or type(event.get('ack_selector')) is not int
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                    or type(event.get('requested_tick')) is not int or event['requested_tick']<0
                    or type(event.get('armed_tick')) is not int or event['armed_tick']<event['requested_tick']
                    or type(event.get('due_tick')) is not int
                    or event['due_tick']!=event['requested_tick']+self._stream_delay(event['ordinal'])
                    or event.get('state') not in ('waiting','asserted','acknowledged','canceled')):
                raise UnsupportedDevice('Finite input stream attempt order/identity/timing is inconsistent')
            if generation==4 and (index!=display_attempts or extras!=self._stream_extra_limit()
                    or not all(acked.values()) or self.event_history[index-1]['state']!='acknowledged'):
                raise UnsupportedDevice('Input close edge requires all declared preceding matching display ACKs')
            if generation==5 and (index!=display_attempts+1
                    or any(prior['state']!='acknowledged' for prior in self.event_history[:display_attempts+1])
                    or self.event_history[display_attempts]['generation']!=4
                    or event['cpu']!=self.event_history[display_attempts]['cpu']
                    or event['requested_tick']<self.event_history[display_attempts]['acknowledged_tick']):
                raise UnsupportedDevice('Input reentry edge requires preceding matching ACKs and the close owner')
            if generation==6 and (index!=display_attempts+2
                    or any(prior['state']!='acknowledged' for prior in self.event_history[:display_attempts+2])
                    or self.event_history[display_attempts+1]['generation']!=5
                    or event['cpu']!=self.event_history[display_attempts+1]['cpu']
                    or event['requested_tick']<self.event_history[display_attempts+1]['acknowledged_tick']):
                raise UnsupportedDevice('Input preview-ready edge requires preceding matching ACKs and the reentry owner')
            if generation==3 and previous_generation==3:
                extras+=1
                prior=self.event_history[index-1]
                if (not all(acked.values()) or prior['state']!='acknowledged'
                        or event['requested_tick']!=prior['acknowledged_tick'] or event['cpu']!=prior['cpu']
                        or type(event.get('source_acknowledged_tick')) is not int
                        or event['source_acknowledged_tick']!=prior['acknowledged_tick']):
                    raise UnsupportedDevice('Finite source0 follow-up lacks the preceding matching ACK or route')
            elif event.get('source_acknowledged_tick') is not None:
                raise UnsupportedDevice('Initial input edge must not claim a preceding source ACK')
            if event['state'] in ('asserted','acknowledged'):
                if type(event.get('asserted_tick')) is not int or event['asserted_tick']<event['due_tick']:
                    raise UnsupportedDevice('Finite input stream assertion precedes its declared delay')
                assertions+=1
            if event['state']=='acknowledged':
                if (type(event.get('acknowledged_tick')) is not int
                        or event['acknowledged_tick']<event['asserted_tick']
                        or type(event.get('acknowledge_selector')) is not int
                        or event['acknowledge_selector']!=identity[2]):
                    raise UnsupportedDevice('Finite input stream history lacks the matching real acknowledgment')
                acks+=1
                if ordinal==1 and generation<=3:acked[generation]=True
            if event['state']=='canceled':canceled+=1
            if index<len(self.event_history)-1 and event['state'] not in ('acknowledged','canceled'):
                raise UnsupportedDevice('Finite input stream has overlapping pending attempts')
            previous_generation=generation
        if (extras!=self.event_extra_attempts or acked!=self.event_startup_acked
                or assertions!=self.event_assertions or acks!=self.event_acknowledgments
                or canceled!=self.event_cancellations
                or self.event_history and (self.last_event!=self.event_history[-1]
                    or self.event_ordinal!=self.last_event['ordinal']
                    or self.event_attempted_generation!=self.last_event['generation'])
                or not self.event_history and (self.last_event is not None or self.event_ordinal!=0)):
            raise UnsupportedDevice('Finite input stream current counters/ACKs differ from its attempt history')
        last=self.last_event
        if (self.event_pending!=(last is not None and last['state']=='asserted')
                or self.event_target is not None and (type(self.event_target) is not int
                    or self.event_target not in (0,1))
                or (self.event_due is not None)!=(last is not None and last['state']=='waiting')
                or self.event_due is not None and (type(self.event_due) is not int
                    or self.event_due!=last['due_tick'] or self.event_target!=last['cpu'])
                or last is not None and (any(type(v) is not int
                    for v in (self.event_source,self.event_irq,self.event_ack_selector))
                    or (self.event_source,self.event_irq,self.event_ack_selector)
                        !=(last['source'],int(last['physical_irq'],16),last['ack_selector']))):
            raise UnsupportedDevice('Finite input stream pending timer/source flag differs from its current attempt')
        followup=self.event_followup
        if self._preview_frame_record() is not None:
            self._validate_preview_frame();return
        if followup is not None and (not isinstance(followup,dict) or self.event_stream_blocked
                or self.event_due is not None or self.event_pending or last is None
                or last['state']!='acknowledged' or last['generation']!=3 or not all(acked.values())
                or self.event_extra_attempts>=self._stream_extra_limit()
                or type(followup.get('generation')) is not int or followup['generation']!=3
                or type(followup.get('route')) is not int or followup['route']!=last['cpu']
                or type(followup.get('acknowledged_tick')) is not int
                or followup['acknowledged_tick']!=last['acknowledged_tick']
                or type(followup.get('after_attempt')) is not int or followup['after_attempt']!=last['attempt']):
            raise UnsupportedDevice('Finite input stream follow-up token lacks an eligible matching ACK')

    def _tick_rec_stream(self,board):
        if self.event_profile in DUE_ROUTE_EVENT_PROFILES:
            self._tick_rec_due_route_stream(board)
            return
        self._audit_rec_stream()
        gates=self._event_gates();identity=self._event_identity()
        target=self._event_route(identity[1]) if gates else None
        if self.event_attempted_generation==3 and (not gates or identity!=(0,0x27,1)
                or self.event_target is not None and target!=self.event_target):
            self._block_rec_stream('Generation3 enable/mask/idle gates or unique route changed')
        if self.event_due is not None:
            if (not gates or target!=self.event_target
                    or identity!=(self.event_source,self.event_irq,self.event_ack_selector)):
                self._cancel_event(board,'Enable/config/idle gates or unique IRQ route changed before edge')
                return
            if board.virtual_ticks<self.event_due:return
            self.event_due=None;self.event_pending=True;self.event_assertions+=1
            self.last_event=dict(self.last_event,state='asserted',asserted_tick=board.virtual_ticks)
            self._save_rec_stream_event()
            board.queue_irq(target,self.event_irq)
            board.record(f'Input source{self.event_source} external edge asserted (unmeasured)',**self.last_event,
                inference='Finite external IRQ edge only; no image/data/status value or software completion')
            return
        if self.event_pending:return
        followup=self.event_followup
        if followup is not None:
            if (self.event_stream_blocked or not gates or target!=followup['route']
                    or self.event_enable_generation!=3 or identity!=(0,0x27,1)):
                self._block_rec_stream('Matching-ACK continuation lost its generation3 gates or route')
                return
            requested_tick=followup['acknowledged_tick']
            self.event_followup=None;self.event_extra_attempts+=1
            ordinal=1+self.event_extra_attempts
        else:
            if (not gates or target is None or self.event_stream_blocked and self.event_enable_generation==3
                    or self.event_enable_generation<=self.event_attempted_generation):return
            self.event_attempted_generation=self.event_enable_generation
            requested_tick=board.virtual_ticks;ordinal=1
        self.event_due=requested_tick+self._stream_delay(ordinal)
        self.event_target=target;self.event_arms+=1;self.event_ordinal=ordinal
        self.event_source,self.event_irq,self.event_ack_selector=identity
        self.last_event=dict(profile=self.event_profile,generation=self.event_enable_generation,
            ordinal=ordinal,attempt=self.event_arms,requested_tick=requested_tick,due_tick=self.event_due,
            armed_tick=board.virtual_ticks,source_acknowledged_tick=requested_tick if ordinal>1 else None,
            cpu=target,source=self.event_source,ack_selector=self.event_ack_selector,
            physical_irq=hex(self.event_irq),state='waiting')
        self.event_history.append(dict(self.last_event))
        board.record(f'Input source{self.event_source} external edge armed (unmeasured)',**self.last_event,
            inference='Finite attempt; chosen delay starts at matching ACK for follow-ups, never a task/PC predicate')

    def _tick_rec_due_route_stream(self,board):
        """Keep the engine timer through temporary kernel IRQ-bank masking.

        This separate declared input policy retains the initial arm's CPU
        owner. It consumes that route only when its external edge is due.
        ACK-paced attempts keep the previous owner's plain ledger identity;
        no disabled route can receive an IRQ and no canceled attempt retries.
        """
        self._audit_rec_stream()
        gates=self._event_gates();identity=self._event_identity()
        if self.event_attempted_generation==3 and (not gates or identity!=(0,0x27,1)):
            self._block_rec_stream('Generation3 enable/mask/idle gates changed')
        if self.event_due is not None:
            if not gates or identity!=(self.event_source,self.event_irq,self.event_ack_selector):
                self._cancel_event(board,'Enable/config/idle gates changed before edge')
                return
            if board.virtual_ticks<self.event_due:return
            target=self._event_route(identity[1])
            if target!=self.event_target:
                self._cancel_event(board,'Unique same-owner IRQ route unavailable at due edge')
                return
            self.event_due=None;self.event_pending=True;self.event_assertions+=1
            self.last_event=dict(self.last_event,state='asserted',asserted_tick=board.virtual_ticks)
            self._save_rec_stream_event()
            board.queue_irq(target,self.event_irq)
            board.record(f'Input source{self.event_source} external edge asserted (unmeasured)',**self.last_event,
                inference='Finite external IRQ edge only; due-time route consumed; no image/data/status value or software completion')
            return
        if self.event_pending:return
        followup=self.event_followup
        if followup is not None:
            if (self.event_stream_blocked or not gates
                    or self.event_enable_generation!=3 or identity!=(0,0x27,1)):
                self._block_rec_stream('Matching-ACK continuation lost its generation3 engine gates')
                return
            target=followup['route'];requested_tick=followup['acknowledged_tick']
            self.event_followup=None;self.event_extra_attempts+=1
            ordinal=1+self.event_extra_attempts
        else:
            if (not gates or self.event_stream_blocked and self.event_enable_generation==3
                    or self.event_enable_generation<=self.event_attempted_generation):return
            display_attempts=3+self._stream_extra_limit()
            if (self.event_enable_generation==4
                    and (len(self.event_history)!=display_attempts or self.event_extra_attempts!=self._stream_extra_limit()
                         or not all(self.event_startup_acked.values())
                         or self.last_event['state']!='acknowledged')):return
            target=self._event_route(identity[1])
            if target is None:return
            if (self.event_enable_generation==5
                    and (len(self.event_history)!=display_attempts+1
                         or any(event['state']!='acknowledged' for event in self.event_history)
                         or self.event_history[-1]['generation']!=4
                         or target!=self.event_history[-1]['cpu'])):return
            if (self.event_enable_generation==6
                    and (len(self.event_history)!=display_attempts+2
                         or any(event['state']!='acknowledged' for event in self.event_history)
                         or self.event_history[-1]['generation']!=5
                         or target!=self.event_history[-1]['cpu'])):return
            self.event_attempted_generation=self.event_enable_generation
            requested_tick=board.virtual_ticks;ordinal=1
        self.event_due=requested_tick+self._stream_delay(ordinal)
        self.event_target=target;self.event_arms+=1;self.event_ordinal=ordinal
        self.event_source,self.event_irq,self.event_ack_selector=identity
        self.last_event=dict(profile=self.event_profile,generation=self.event_enable_generation,
            ordinal=ordinal,attempt=self.event_arms,requested_tick=requested_tick,due_tick=self.event_due,
            armed_tick=board.virtual_ticks,source_acknowledged_tick=requested_tick if ordinal>1 else None,
            cpu=target,source=self.event_source,ack_selector=self.event_ack_selector,
            physical_irq=hex(self.event_irq),state='waiting')
        self.event_history.append(dict(self.last_event))
        board.record(f'Input source{self.event_source} external edge armed (unmeasured)',**self.last_event,
            inference='Finite attempt; timer/ACK origin survives temporary IRQ-bank masks; same owner must be uniquely enabled when due')

    # ---- Opt-in continuous source clock -------------------------------------
    STREAM_IDENTITIES={0x10003:(0,0x27,1),0x10203:(0,0x27,1),0x10002:(1,0x29,0),0x10202:(1,0x29,0)}
    STREAM_HISTORY=64

    def enable_stream(self,*,period_ticks,first_delay_ticks=None,source1=False,companion_source1=False):
        """Select the host-declared continuous source clock.

        Only external edges are produced. Each edge is gated by the firmware's
        own enable latches/mask and by one enabled IRQ route, and is cleared
        only by the stock selector ACK. An edge whose predecessor is still
        unacknowledged is dropped, not queued. No registers, callbacks or
        messages are written. Existing finite profiles are unaffected until
        this is called, and their ledgers are retained unchanged.
        """
        if self.event_profile not in REC_STREAM_EVENT_PROFILES:
            raise UnsupportedDevice('Continuous stream requires a REC stream event profile')
        if isinstance(period_ticks,bool) or not isinstance(period_ticks,int) or period_ticks<1000:
            raise UnsupportedDevice('Stream period must be an integer of at least 1000 virtual ticks')
        if self.stream is not None:
            raise UnsupportedDevice('Continuous stream is already enabled')
        now=self.board.virtual_ticks
        delay=period_ticks if first_delay_ticks is None else first_delay_ticks
        self.stream=dict(profile=STREAM_EVENT_PROFILE,period_ticks=period_ticks,source1=bool(source1),companion_source1=bool(companion_source1),
            frame=0,next_due=now+delay,pending=None,edges=0,acks=0,dropped=0,gated=0,
            enabled_tick=now,last_assert_tick=None,last_ack_tick=None,history=[])
        self.board.record('Continuous input stream enabled',**{k:v for k,v in self.stream.items() if k!='history'},
            limits='External edges only, gated by firmware enable/mask/route and cleared by stock ACK; physical cadence unmeasured')
        return dict(self.stream)

    def disable_stream(self):
        if self.stream is not None:
            self.board.record('Continuous input stream disabled',frames=self.stream['frame'])
        self.stream=None

    def set_stream_period(self,period_ticks):
        if self.stream is None or isinstance(period_ticks,bool) or not isinstance(period_ticks,int) or period_ticks<1000:
            raise UnsupportedDevice('Stream period change requires an enabled stream and integer period>=1000')
        self.stream['period_ticks']=period_ticks

    def _stream_ack(self,value):
        s=self.stream
        if s is None:return False
        companion=s.get('companion')
        if companion is not None and value==companion['ack_selector']:
            s['companion']=None;s['companion_acks']=s.get('companion_acks',0)+1
            return True
        if s['pending'] is None or value!=s['pending']['ack_selector']:return False
        pending=s['pending'];s['pending']=None;s['acks']+=1;s['last_ack_tick']=self.board.virtual_ticks
        s['history'][-1]['acknowledged_tick']=self.board.virtual_ticks
        host=getattr(self.board,'sensor_stream',None)
        if host is not None:host.on_ack(pending,self.board.virtual_ticks)
        return True

    def _stream_identity(self):
        if not (self.registers.get(0x48,0)&1 and not self.registers.get(0x70,0)&1
                and not self.busy and self.pending_command is None and self.command_armed is None
                and not self.phase_busy):
            return None
        mask=self.registers.get(0x38c,0)
        # Bit16 enables the routing; bit0 selects source0 (IRQ 0x27, selector 1),
        # otherwise bit1 selects source1 (IRQ 0x29, selector 0). Seen: 10003/10203
        # (REC live view), 10201 (generation-4 AF acquisition), 10002/10202.
        if not mask&0x10000:return None
        if mask&1:return (0,0x27,1)
        return (1,0x29,0) if mask&2 else None

    def _tick_stream(self,board):
        s=self.stream
        if board.virtual_ticks<s['next_due']:return
        period=s['period_ticks']
        # Fixed cadence; whole periods the host ran past are counted as dropped.
        behind=(board.virtual_ticks-s['next_due'])//period
        s['next_due']+=period*(behind+1)
        s['dropped']+=behind
        identity=self._stream_identity()
        if identity is None or identity[0]==1 and not s['source1']:
            s['gated']+=1;return
        if s['pending'] is not None:
            s['dropped']+=1;return
        source,irq,selector=identity
        route=self._event_route(irq)
        if route is None:
            s['gated']+=1;return
        host=getattr(board,'sensor_stream',None)
        ready=getattr(host,'frame_ready',None)
        if ready is not None and not ready(source,board.virtual_ticks):
            s['gated']+=1;return
        s['frame']+=1;s['edges']+=1;s['last_assert_tick']=board.virtual_ticks
        s['pending']=dict(frame=s['frame'],source=source,irq=irq,ack_selector=selector,cpu=route,
            asserted_tick=board.virtual_ticks)
        s['history'].append(dict(s['pending'],acknowledged_tick=None))
        del s['history'][:-self.STREAM_HISTORY]
        if host is not None:host.on_edge(dict(s['pending']),board.virtual_ticks)
        board.queue_irq(route,irq)
        # Optional companion source1 edge with the source0 frame, only while
        # the firmware's mask selects bit1 and exactly one CPU enables IRQ 0x29.
        if s.get('companion_source1') and source==0 and self.registers.get(0x38c,0)&2:
            if s.get('companion') is not None:
                s['companion_dropped']=s.get('companion_dropped',0)+1
            else:
                route1=self._event_route(0x29)
                if route1 is not None:
                    s['companion']=dict(frame=s['frame'],source=1,irq=0x29,ack_selector=0,cpu=route1,
                        asserted_tick=board.virtual_ticks)
                    s['companion_edges']=s.get('companion_edges',0)+1
                    board.queue_irq(route1,0x29)

    def _ack_xm1_clock(self, selector):
        source = PhaseSource.PRIMARY if selector == 1 else PhaseSource.SECONDARY
        edge = self.phase_clock_pending.pop(source, None)
        if edge is None:
            return
        self.registers[0x384] &= ~edge.status_mask
        self.event_pending = bool(self.phase_clock_pending)
        self.event_acknowledgments += 1
        self.last_event = dict(profile=XM1_CLOCK_PROFILE, source=int(source), cpu=edge.cpu,
                               asserted_tick=edge.asserted_tick, generation=edge.generation,
                               state='acknowledged', acknowledged_tick=self.board.virtual_ticks,
                               acknowledge_selector=selector)
        self.board.record('X-M1 phase-clock source acknowledged', **self.last_event)

    def _tick_xm1_clock(self, board):
        from x70_native._native import tick_xm1_clock
        tick_xm1_clock(self, board, PhaseClockEdge, PhaseSource)

    def _tick_event(self,board):
        if self.event_profile == XM1_CLOCK_PROFILE:
            self._tick_xm1_clock(board)
            return
        if self.stream is not None:
            self._tick_stream(board);return
        if self.event_profile is None:return
        if self._preview_frame_record() is not None:
            self._tick_preview_frame(board);return
        if self.event_profile in REC_STREAM_EVENT_PROFILES:
            self._tick_rec_stream(board)
            return
        gates=self._event_gates()
        identity=self._event_identity()
        target=self._event_route(identity[1]) if gates else None
        if self.event_due is not None:
            if (not gates or target!=self.event_target
                    or identity!=(self.event_source,self.event_irq,self.event_ack_selector)):
                self._cancel_event(board,'Enable/config/idle gates or unique IRQ route changed before edge')
                return
            if board.virtual_ticks<self.event_due:return
            self.event_due=None;self.event_pending=True
            self.event_assertions+=1
            self.last_event=dict(self.last_event,state='asserted',asserted_tick=board.virtual_ticks)
            board.queue_irq(target,self.event_irq)
            board.record(f'Input source{self.event_source} external edge asserted (unmeasured)',**self.last_event,
                inference='Chosen external IRQ edge only; event status bits, image/data timing and pixels remain unresolved')
            return
        if (not gates or target is None or self.event_pending
                or self.event_enable_generation<=self.event_attempted_generation):return
        self.event_attempted_generation=self.event_enable_generation
        self.event_due=board.virtual_ticks+self.event_ticks
        self.event_target=target;self.event_arms+=1
        self.event_source,self.event_irq,self.event_ack_selector=identity
        self.last_event=dict(profile=self.event_profile,generation=self.event_enable_generation,
            requested_tick=board.virtual_ticks,due_tick=self.event_due,cpu=target,
            source=self.event_source,ack_selector=self.event_ack_selector,
            physical_irq=hex(self.event_irq),state='waiting')
        board.record(f'Input source{self.event_source} external edge armed (unmeasured)',**self.last_event,
            inference='Positive configurable virtual first-edge delay, independent of display/phase clocks; not physical capture')

    def arm_still_raw(self,profile,*,scene,codec_profile,delay_ticks=1000):
        from . import afs_still_readout
        return afs_still_readout.arm(self,profile,scene=scene,codec_profile=codec_profile,delay_ticks=delay_ticks)

    def still_raw_summary(self):
        from . import afs_still_readout
        return afs_still_readout.summary(self)

    def tick(self,board):
        if hasattr(self,'processing_activation') and 'data' in self.processing_activation:
            from . import processing_image_delivery
            processing_image_delivery.tick(self,board)
        if self.busy and board.virtual_ticks>=self.reset_due:
            self.registers[0]|=1
            self.busy=False;self.reset_due=None;self.reset_completions+=1
            board.record('Input processing reset acknowledged',control=hex(self.registers[0]),
                profile=self.RESET_PROFILE,
                inference='Only bit0 transition required by actual92205C; physical delay and other status fields unknown')
        if self.pending_command is not None and board.virtual_ticks>=self.pending_command['due_tick']:
            self.last_command=dict(self.pending_command,state='completed',completed_tick=board.virtual_ticks)
            self.pending_command=None;self.command_armed=None
            self.dma_busy=False
            self.registers[0x94]&=~1
            self.command_completions+=1
            board.record('Input command transmission completed (inferred)',
                profile=self.command_profile,control=hex(self.registers[0x94]),
                packed_sha256=self.last_command['packed_sha256'],
                inference='Chosen virtual TX completion clears94 bit0; no sensor effects, IRQ, pixel, DMA output or guest RAM write')
        for offset,item in list(self.pending_phases.items()):
            if board.virtual_ticks<item['due_tick']:continue
            self.registers[offset]=(self.registers[offset]&~3)|3
            self.last_phases[offset]=dict(item,completed_tick=board.virtual_ticks,
                acknowledged=hex(self.registers[offset]))
            del self.pending_phases[offset];self.phase_completions+=1
            board.record('Input inactive phase acknowledged (inferred)',**self.last_phases[offset],
                inference='Only low2→3 as required by inactive stock quiescence poll; no image/IRQ/DMA/frame or receiver effects')
        self.phase_busy=bool(self.pending_phases)
        if hasattr(self,'still_return_phases'):
            from . import input_still_return
            input_still_return.tick(self)
        self._tick_event(board)
        if getattr(self,'raw_readout',None) is not None:
            from . import afs_still_readout
            afs_still_readout.tick(self,board)

    def arm_still_return_phases(self,*,profile):
        from . import input_still_return
        return input_still_return.arm(self,profile)

    def arm_processing_activation(self,profile,*,caller_address=None):
        from . import input_processing_activation
        return input_processing_activation.arm(self,profile,caller_address=caller_address)

    def processing_activation_summary(self):
        from . import input_processing_activation
        return input_processing_activation.summary(self)

    def arm_processing_output(self,profile,*,delay_ticks=1024,idle_ticks=16):
        from . import processing_image_delivery
        return processing_image_delivery.arm(self,profile,delay_ticks=delay_ticks,idle_ticks=idle_ticks)

    def processing_output_summary(self):
        from . import processing_image_delivery
        return processing_image_delivery.summary(self)

    def snapshot(self):
        if getattr(self,'raw_readout',None) is not None:self.still_raw_summary()
        result=dict(name=type(self).__name__,base=hex(self.BASE),
            registers={hex(self.BASE+offset):hex(value) for offset,value in self.registers.items()},
            busy=self.busy,reset_due=self.reset_due,reset_requests=self.reset_requests,
            reset_completions=self.reset_completions,reset_ticks=self.reset_ticks,
            reset_profile=self.RESET_PROFILE,writes=self.writes,last_rejected=self.last_rejected,
            command_profile=self.command_profile,command_ticks=self.command_ticks,
            dma_busy=self.dma_busy,
            command_armed=self.command_armed,pending_command=self.pending_command,last_command=self.last_command,
            command_requests=self.command_requests,command_completions=self.command_completions,
            phase_profile=self.phase_profile,phase_ticks=self.phase_ticks,phase_busy=self.phase_busy,
            command_cancellations=self.command_cancellations,
            pending_phases={hex(offset):item for offset,item in self.pending_phases.items()},
            last_phases={hex(offset):item for offset,item in self.last_phases.items()},
            phase_requests=self.phase_requests,phase_completions=self.phase_completions,
            stream=self.stream,
            event_profile=self.event_profile,event_ticks=self.event_ticks,
            event_enable_generation=self.event_enable_generation,event_attempted_generation=self.event_attempted_generation,
            event_due=self.event_due,event_target=self.event_target,event_pending=self.event_pending,
            event_source=self.event_source,event_irq=self.event_irq,event_ack_selector=self.event_ack_selector,
            event_arms=self.event_arms,event_assertions=self.event_assertions,
            event_acknowledgments=self.event_acknowledgments,event_cancellations=self.event_cancellations,
            last_event=self.last_event,
            event_startup_acked=self.event_startup_acked,event_extra_attempts=self.event_extra_attempts,
            event_ordinal=self.event_ordinal,event_followup=self.event_followup,
            event_stream_blocked=self.event_stream_blocked,
            event_stream_cancellations=self.event_stream_cancellations,event_history=self.event_history,
            event_extra_attempt_limit=self._stream_extra_limit() if self.event_profile in REC_STREAM_EVENT_PROFILES else 0,
            event_final_gap_multiplier=REC_DISPLAY_FINAL_GAP_MULTIPLIER if self.event_profile in REC_DISPLAY_EVENT_PROFILES else 1,
            still_raw=self.still_raw_summary() if getattr(self,'raw_readout',None) is not None else None,
            limits=['Initial register zeros are explicit unmeasured fixtures.',
                'The numbered legacy opt-in contracts below derive from X70 1.10; they do not establish X-M1 hardware equivalence.',
                'X-M1 configured clocks retain peripheral status across CPU IRQ masking; routing resumes only with one enabled bank. Source0 period and source1 count-to-tick relation are declared, unmeasured laws.',
                'The X-M1 phase profile preserves upper fields across low3→2→3 and admits active input48; shared13C enable and inactive70 remain required.',
                'RAW channel2/3 retain whole9239D4/923B60 descriptor fields and920ADC IRQ masks;920A3C ACKs are write-only. Optional explicitly armed finite channel2 row DMA/IRQ24 uses a declared monochrome scene; source clocks and firmware completion remain separate.',
                'Image input/processing interpretation derives from dimensions and DMA-address setters; exact engine identity unknown.',
                'Clear-only global control and bounded passive global configuration retain stock writes.',
                'Only write0-bit0 -> bit0-set reset acknowledgment is inferred; virtual delay is unmeasured.',
                'Channel starts, status, DMA, image results, interrupts and unrecognized registers are unsupported outside the exact opt-in write-only packet.',
                'The original command profile accepts only the exact256-record reset-packet hash. The separate packed-writes profile accepts1..512 mode0/unit3/channel0 opcode81 records with five zero padding bytes.',
                'Both profiles infer delayed virtual transmission/94-bit0 clear; neither models a measured physical completion or command-receiver effect.',
                'Guest-written enable latches and indexed phase configs are retained; there is no auto enable/disable, stream or pixel result.',
                'Optional inactive phase acknowledgment accepts only exact3→2 before input enable with shared13C=1; delayed low2→3 is inferred, never image completion.',
                    'Optional distinct source1 profile supplies one unmeasured delayed IRQ29 edge per genuine48 bit0 enable generation, only with48 on/70 off/38C10002, idle engines and one enabled route.',
                    'Separate dual-source profile additionally permits source0 IRQ27 at exact38C10003 with selector1 acknowledgment; source1 remains IRQ29/selector0. At most one attempted edge of either source per genuine enable generation.',
                    'Separate finite REC-startup profile permits only generation1/mask10002 source1, generation2/mask10003 source0 and generation3/mask10203 source0. Later generations and other pairs have no edge identity; no continuous sensor cadence is inferred.',
                    f'Separate finite REC stream experiment preserves those three startup identities, then permits at most {self._stream_extra_limit()} additional source0 IRQ27 attempts in the same generation3/mask10203, each delayed from the preceding matching real388=1 ACK. All three initial ACKs are required. Budget is consumed at arming; disable/mask/idle/route loss ends continuation without clearing any kernel IRQ or inventing a replacement. No physical cadence, image, DMA or downstream work completion is established.',
                    'The separate four-followup display profile delays only ordinal5 by8 times event_ticks; default ordinal5 supplies no third image/histogram payload.',
                    'The card-late-histogram alias preserves all finite edge identities; histogram/ImageWork may project ordinal5, retaining their two-result limits.',
                    'Only the separate card-staging profile adds one generation3/source0 ordinal6 after an eightfold matching-ACK delay. It supplies no third image/histogram; later close/reentry/ready edges require all shifted prior ACKs on the same owner.',
                'Pending external-edge timers are capturable device state, not scheduled closures. Legacy profiles cancel on pre-assertion gate/route loss; the separate due-route display/close profiles retain their timer during temporary IRQ masking and require the same unique enabled owner at due assertion. Engine/source/configuration/generation loss still cancels immediately; canceled attempts never retry.',
                'The separate close profile preserves seven display edges then permits exactly one generation4/mask10202 source1 IRQ29/selector0 edge after their real matching ACKs. Later enables, source0 follow-ups, status bits, pixel/DMA output and callback substitution remain unsupported.',
                'The separate REC reentry profile adds only generation5/mask10002 source1 IRQ29/selector0, after all eight prior real matching ACKs and on the preceding close owner. No later edge identity, source0 continuation, second data lifecycle or callback substitution is supplied.',
                'The separately selected preview-ready profile extends that ledger with exactly one generation6/mask10003 source0 IRQ27/selector1 edge, after nine matching real ACKs and on the same reentry owner. Later generations, follow-ups and a second image/data lifecycle remain unsupported.',
                'Explicit arm_preview_frame selects one separate generation6/source0 post-arm clock edge only after all ten ACKs and a complete new producer22-access request. Its plain marker retains the original ledger, requires the live route at due assertion, and supplies no data, status, software callback or result.',
                    'Only the selected source matching acknowledgment clears its virtual flag: source1 selector0, source0 selector1. In the original single/dual/startup profiles neither ACK rearms; a genuine input0 disable→enable is required. Only the separate finite stream experiment may request its next bounded source0 attempt from that matching ACK.',
                    'Status384 reads while the external timer/flag is pending remain unsupported; no physical status value for bits0/1/9 is invented.',
                'No external command receiver, register effects, sensor response, image or guest-memory DMA result is modeled.'])
        if hasattr(self,'still_return_phases'):
            from . import input_still_return
            result['still_return_phases']=input_still_return.summary(self)
        if hasattr(self,'processing_activation'):
            result['processing_activation']=self.processing_activation_summary()
        return result
