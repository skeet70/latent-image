"""Run the captured ROM and stock loader against the explicit board model."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
from uuid import uuid4
import webbrowser
from time import perf_counter

from .board import Board, ROOT, RAM_SIZE
from .boards.profile import ResolvedProfile, resolve, thaw
from .boards.wiring import wire_graph
from .boot_devices import BootControls,normal_power_on_gpio_profile,SHUTTER_IRQ_PROFILE
from .interrupts import KernelInterrupts
from .loader_devices import (NandController, FujiDecompressor,
                             LoaderMemoryDMA, native_flash_from_dat)
from .power_devices import PowerDevices,normal_power_on_profile,absent_external_flash_profile
from .captured_mcu_profile import captured_mcu_profile
from .audio_devices import AudioSerialControl
from .graphics_devices import GraphicsAccelerator
from .hdmi_devices import HDMIStartup
from .output_devices import OutputControl
from .display_timing import DisplayTiming
from .display_formatter import DisplayFormatter
from .pulse_devices import PulseOutputs
from .spi2_devices import SPI2Serial
from .lens_endpoint import LensEndpoint, RESPONSE_PROFILES as LENS_RESPONSE_PROFILES
from .lens_homing_fixture import HOMING_PROFILE, LensHomingFixture, CONTINUATION_PROFILES as LENS_CONTINUATION_PROFILES
from .sd_card import SDCardController, MediaImage, load_raw_image, PROFILES as SD_CARD_PROFILES
from .wifi_sdio import WiFiSDIOController, PROFILES as WIFI_SDIO_PROFILES
from .control_bus import ControlBus,PROFILES as CONTROL_BUS_PROFILES
from .secondary_control_bus import SecondaryControlBus,PROFILES as SECONDARY_BUS_PROFILES
from .fe80_devices import FE80Control
from .usb_devices import USBControl
from .input_processing_devices import (InputProcessingControl,REC_DISPLAY_EVENT_PROFILES,CARD_STAGING_EVENT_PROFILE,PROFILES as INPUT_PROCESSING_PROFILES,
                                      PHASE_PROFILES as INPUT_PHASE_PROFILES,EVENT_PROFILES as INPUT_EVENT_PROFILES)
from .input_auxiliary_devices import InputAuxiliaryControl,CONTROL_PROFILES as AUXILIARY_CONTROL_PROFILES
from .producer_devices import (ProducerControl,STOP_PROFILES as PRODUCER_STOP_PROFILES,
                               DATA_PROFILES as PRODUCER_DATA_PROFILES,HISTOGRAM_PROFILES)
from .image_parameter_devices import ImageParameterControl, DeclaredUIConversion, UIConversionProfile
from .image_work_devices import (ImageWorkControl, STOP_PROFILES as IMAGE_WORK_STOP_PROFILES,
                                 DATA_PROFILES as IMAGE_WORK_DATA_PROFILES)
from .face_analysis_devices import FaceAnalysisControl, STOP_PROFILES as FACE_STOP_PROFILES
from .af_extraction import AFExtractionControl, PROFILES as AF_EXTRACTION_PROFILES
from .afs_frame_source import AfsFrameSource, PROFILES as AFS_FRAME_SOURCE_PROFILES
from .scanout import DisplayScanout, RectangleProfile, UnsupportedScanout
from .storage_profile import (attach_captured_storage_profile, attach_captured_user_journals_profile,
                             owner_mcu_profile, FIRMWARE_SHA256 as CAPTURE_FIRMWARE_SHA256)
from .checkpoint import CheckpointError
from x70_native.checkpoint import capture
from x70_native._native import FFF63Engine, FFF63_PROFILES, FFF48Engine, MODE_DEVICE_PROFILE


def make_board(entry='rom', quantum=1000,storage_profile='erased',graphics_profile=None,
               *, backdrop_profile=None,rectangle_profile=None,display_frame_ticks=None,copy_profile=None,clear_profile=None,irq_tie_profile=None,
               mcu_profile=None,user_journals=None,phase_profile='bit16',spi0_input_profile=None,
               control_bus_profile=None,secondary_bus_profile=None,input_processing_profile=None,input_phase_profile=None,
               input_event_profile=None,input_event_ticks=400000,counter_input_profile=None,pulse_timer_profile=None,
               lens_response_profile=None,lens_response_ticks=400000,lens_continuation_profile=None,
               sd_card_profile=None,sd_card_present=False,sd_card_transfer_ticks=100,sd_card_sectors=0x800000,
               sd_card_image=None,
               wifi_sdio_profile=None,wifi_sdio_transfer_ticks=100,
               auxiliary_control_profile=None,auxiliary_control_ticks=2,
               producer_stop_profile=None,producer_stop_ticks=2,cache_instructions=True,
               producer_data_profile=None,producer_data_fixture=None,producer_data_ticks=400000,producer_ack_ticks=2,
               image_work_stop_profile=None,image_work_stop_ticks=2,
               image_work_data_profile=None,image_work_data_ticks=400000,
               histogram_profile=None,histogram_data_ticks=400000,
               movrsz_profile=None,movrsz_ticks=400000,face_stop_profile=None,
               af_extraction_profile=None,af_extraction_ticks=400000,af_extraction_stop_ticks=2,
               shutter_irq_profile=None,afs_frame_source_profile=None,afs_frame_source_ticks=400000,still_pipeline_profile=None,
               instructions_per_tick=1,settings=None,firmware_profile=None,ui_conversion_profile=None,
               fff63_profile=None,mode_devices_profile=None):
    factory_options={key: str(value) if isinstance(value, Path) else value
                     for key, value in locals().items()
                     if key != 'sd_card_image' and (key not in ('settings','firmware_profile') or value is not None) and (key not in ('still_pipeline_profile','instructions_per_tick') or value not in (None,1))}
    ui_conversion_profile = UIConversionProfile.parse(ui_conversion_profile)
    if ui_conversion_profile is None:
        factory_options.pop('ui_conversion_profile', None)
    else:
        factory_options['ui_conversion_profile'] = ui_conversion_profile.value
    if fff63_profile is None:
        factory_options.pop('fff63_profile', None)
    elif fff63_profile not in FFF63_PROFILES:
        raise ValueError('Unknown FFF63 engine profile')
    if mode_devices_profile is None:
        factory_options.pop('mode_devices_profile', None)
    elif mode_devices_profile != MODE_DEVICE_PROFILE:
        raise ValueError('Unknown mode device profile')
    if 'instructions_per_tick' in factory_options:
        from x70_native.board import normalize_instructions_per_tick, ratio_option
        factory_options['instructions_per_tick']=ratio_option(normalize_instructions_per_tick(instructions_per_tick))
    rectangle_profile = None if rectangle_profile is None else RectangleProfile(rectangle_profile)
    if (afs_frame_source_profile not in (None,*AFS_FRAME_SOURCE_PROFILES)
            or type(afs_frame_source_ticks) is not int or not 1<=afs_frame_source_ticks<1<<64):
        raise ValueError('AF-S frame source requires a named profile and positive bounded virtual delay')
    if lens_continuation_profile not in (None,*LENS_CONTINUATION_PROFILES):
        raise ValueError('Unknown lens continuation profile')
    if lens_continuation_profile is not None and lens_response_profile!=HOMING_PROFILE:
        raise ValueError('Lens continuation requires the full startup homing profile')
    if type(sd_card_present) is not bool or sd_card_profile not in (None,*SD_CARD_PROFILES):
        raise ValueError('SD card requires a named profile and explicit boolean presence')
    if sd_card_present and sd_card_profile is None:
        raise ValueError('SD presence requires an explicitly selected card profile')
    if sd_card_image is not None and (sd_card_profile is None or not sd_card_present):
        raise ValueError('SD image initialization requires an explicitly present profiled card')
    if (wifi_sdio_profile not in (None,*WIFI_SDIO_PROFILES)
            or type(wifi_sdio_transfer_ticks) is not int or wifi_sdio_transfer_ticks<1):
        raise ValueError('WiFi SDIO requires a named profile and positive virtual transfer ticks')
    if firmware_profile is not None and not isinstance(firmware_profile,ResolvedProfile):
        raise TypeError('Boot firmware_profile must be an explicitly resolved ResolvedProfile')
    if firmware_profile is not None:
        from x70_native.portable_checkpoint import _profile_options
        factory_options['firmware_profile']=_profile_options(firmware_profile)
    profile=(resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
             if firmware_profile is None else firmware_profile)
    if profile.board.get('execution_scope')=='isolated-driver':
        raise ValueError('Driver-only profile cannot construct a boot board')
    if (profile.manifest['source_sha256']!=CAPTURE_FIRMWARE_SHA256 and
            (storage_profile=='captured' or mcu_profile is not None or user_journals is not None or settings=='owner')):
        raise ValueError('Selected captured fixtures differ from the resolved firmware; use explicit sibling backing')
    board=Board(entry=entry,quantum=quantum,profile=profile,cache_instructions=cache_instructions,
                instructions_per_tick=instructions_per_tick)
    from .boards.cpu_parameters import tcm_parameters
    configure_tcm=getattr(board._native,'configure_tcm',None)
    if configure_tcm is not None:configure_tcm(tcm_parameters(profile.soc))
    board._portable_factory_options=factory_options
    gpio_profile=dict(profile.board['inputs']['gpio'])
    gpio_profile['gpio_inputs']={int(k):v for k,v in gpio_profile['gpio_inputs'].items()}
    from .boards.service_parameters import service_parameters
    service=service_parameters(profile)
    if profile.board['inputs']['usb_connected'] is not False and 'usb' not in service:
        raise ValueError('USB-connected profile requires its explicit endpoint binding')
    if 'control_bus0' in service and control_bus_profile is not None:
        raise ValueError('Profile-selected control bus cannot be overridden by another reply profile')
    if 'display_timing' in service and (display_frame_ticks is not None or phase_profile!='bit16'):
        raise ValueError('Profile-selected display timing cannot be overridden by another clock profile')
    callbacks=dict(read_bytes=board.read_memory,write_bytes=board.write_memory,
                   schedule=board.schedule,record=board.record)
    irq_targets=lambda line:[n for n,bank in enumerate(board.interrupts.banks)
                             if bank.enabled & (1<<line)]
    power_profile=dict(profile.board['inputs']['power'])
    power_profile['adc_samples']=tuple(power_profile['adc_samples'])
    power_profile['packet']=bytes.fromhex(profile.board['inputs']['mcu_packet_hex'])
    if profile.board['inputs']['usb_connected'] is True:
        power_profile['usb_connected']=True
    if spi0_input_profile is not None:
        absent=absent_external_flash_profile()
        if spi0_input_profile!=absent['spi0_input_profile']:
            raise ValueError('Unknown SPI0 input profile')
        power_profile.update(absent)
    captured=None
    if mcu_profile is not None or settings=='owner':
        if mcu_profile not in (None,'captured-backup'):
            raise ValueError('Unknown MCU input profile')
        captured=captured_mcu_profile() if settings!='owner' else owner_mcu_profile()
        power_profile.update(packet=captured['packet'],profile_name=captured['profile_name'])
    controls=power=lens=None
    homing=None
    builders={
        'PowerDevices': lambda **refs: PowerDevices(board,firmware_profile=profile,**power_profile),
        'BootControls': lambda **refs: BootControls(board,shutter_irq_profile=shutter_irq_profile,
                                      **gpio_profile),
        'KernelInterrupts': lambda **refs: KernelInterrupts(board,tie_profile=irq_tie_profile),
        'NandController': lambda **refs: NandController(
        profile.flash,
        raise_irq=board.queue_irq,**callbacks),
        'FujiDecompressor': lambda **refs: FujiDecompressor(raise_irq=board.queue_irq,
        movrsz_profile=movrsz_profile,movrsz_ticks=movrsz_ticks,
        irq_targets=irq_targets,**callbacks),
        'LoaderMemoryDMA': lambda **refs: LoaderMemoryDMA(raise_irq=board.queue_irq,
        irq_targets=irq_targets,**callbacks),
        'USBControl': lambda **refs: USBControl(board,refs['power'],
                                  **({'endpoint_contract':service['usb']} if 'usb' in service else {})),
        'AudioSerialControl': lambda **refs: AudioSerialControl(board),
        'PulseOutputs': lambda **refs: PulseOutputs(board,counter_input_profile=counter_input_profile,timer_profile=pulse_timer_profile),
        'LensEndpoint': lambda **refs: LensEndpoint(board,response_profile=lens_response_profile,response_ticks=lens_response_ticks,
                                   homing_fixture=homing),
        'SPI2Serial': lambda **refs: SPI2Serial(board,endpoint=refs['endpoint']),
        'ControlBus': lambda **refs: ControlBus(board,**service.get('control_bus0',{'profile':control_bus_profile})),
        'SecondaryControlBus': lambda **refs: SecondaryControlBus(board,refs['power'],profile=secondary_bus_profile),
        'FE80Control': lambda **refs: FE80Control(board,**service.get('fe80',{})),
        'InputProcessingControl': lambda **refs: InputProcessingControl(board,command_profile=input_processing_profile,phase_profile=input_phase_profile,
                                        event_profile=input_event_profile,event_ticks=input_event_ticks),
        'InputAuxiliaryControl': lambda **refs: InputAuxiliaryControl(board,control_profile=auxiliary_control_profile,control_ticks=auxiliary_control_ticks),
        'ProducerControl': lambda **refs: ProducerControl(board,stop_profile=producer_stop_profile,stop_ticks=producer_stop_ticks,
        data_profile=producer_data_profile,data_fixture=producer_data_fixture,
        data_ticks=producer_data_ticks,ack_ticks=producer_ack_ticks,
        histogram_profile=histogram_profile,histogram_data_ticks=histogram_data_ticks,
        rotation_limit=(5 if input_event_profile==CARD_STAGING_EVENT_PROFILE
                        else 4 if input_event_profile in REC_DISPLAY_EVENT_PROFILES else 3)),
        'ImageParameterControl': lambda **refs: ImageParameterControl(board),
        'ImageWorkControl': lambda **refs: ImageWorkControl(board,stop_profile=image_work_stop_profile,stop_ticks=image_work_stop_ticks,
        data_profile=image_work_data_profile,data_ticks=image_work_data_ticks),
        'FaceAnalysisControl': lambda **refs: FaceAnalysisControl(board,stop_profile=face_stop_profile,mode_profile=mode_devices_profile),
        'HDMIStartup': lambda **refs: HDMIStartup(board),
        'OutputControl': lambda **refs: OutputControl(board),
        'GraphicsAccelerator': lambda **refs: GraphicsAccelerator(raise_irq=board.queue_irq,ram_bytes=len(board.ram),copy_profile=copy_profile,
        clear_profile=clear_profile,
        irq_targets=irq_targets,**callbacks),
        'DisplayScanout': lambda **refs: DisplayScanout(read_bytes=board.read_memory,
        schedule=board.schedule,record=board.record,graphics_profile=graphics_profile,
        backdrop_profile=backdrop_profile,rectangle_profile=rectangle_profile),
        'DisplayTiming': lambda **refs: DisplayTiming(board,**service.get('display_timing',
                                     {'frame_period_ticks':display_frame_ticks,'phase_profile':phase_profile})),
        'DisplayFormatter': lambda **refs: DisplayFormatter(board),
    }
    def after_attach(kind, device):
        nonlocal controls, power, lens, homing
        if kind=='BootControls':
            controls=device
            if sd_card_profile is not None:
                if sd_card_present:
                    controls.set_gpio_input_pin(12,7,0,source=sd_card_profile)
                controller=SDCardController(board,controls,profile=sd_card_profile,
                    transfer_ticks=sd_card_transfer_ticks,sectors=sd_card_sectors)
                if sd_card_image is not None:
                    controller.image=MediaImage(**load_raw_image(controller.card,sd_card_image))
                board.attach(controller)
            if wifi_sdio_profile is not None:
                board.attach(WiFiSDIOController(board,controls,profile=wifi_sdio_profile,
                                               transfer_ticks=wifi_sdio_transfer_ticks))
        elif kind=='KernelInterrupts':
            board.interrupts=device
        elif kind=='NandController':
            board.nand=device
        elif kind=='LoaderMemoryDMA':
            if captured is not None:
                board.record('Captured MCU RAM packet used as inferred input',profile=captured['provenance'])
        elif kind=='PowerDevices':
            power=device
        elif kind=='PulseOutputs':
            if lens_response_profile==HOMING_PROFILE:
                if counter_input_profile!=PulseOutputs.COUNTER_LENS_PROFILE:
                    raise ValueError('Full lens startup requires the named lens feedback counter profile')
                homing=board.attach(LensHomingFixture(board,response_ticks=lens_response_ticks,
                                                     continuation_profile=lens_continuation_profile))
        elif kind=='LensEndpoint':
            lens=device
        elif kind=='FaceAnalysisControl':
            if af_extraction_profile is not None:
                board.attach(AFExtractionControl(board,profile=af_extraction_profile,
                    data_ticks=af_extraction_ticks,stop_ticks=af_extraction_stop_ticks))
            if afs_frame_source_profile is not None:
                board.attach(AfsFrameSource(board,profile=afs_frame_source_profile,ticks=afs_frame_source_ticks))
        elif kind=='GraphicsAccelerator':
            if storage_profile=='captured':
                board.storage_profile=attach_captured_storage_profile(board.nand,settings=settings)
                board.record('Per-unit storage profile attached',profile=board.storage_profile)
            elif storage_profile=='erased':
                board.storage_profile={'profile':'erased','note':'Managed configuration partition is an erased fixture.'}
            else:
                raise ValueError('Unknown storage profile')
            if user_journals is not None or settings=='owner':
                board.storage_profile['user_journals']=attach_captured_user_journals_profile(
                    board.nand,source='owner' if settings=='owner' else user_journals)
                board.record('Captured user journals attached',profile=board.storage_profile['user_journals'])
        elif kind=='DisplayScanout':
            board.display=device
        elif kind=='DisplayTiming':
            board.display_timing=device
        elif kind=='DisplayFormatter':
            board.display_formatter=device
    instances={}
    wire_graph(board, profile.board, builders, after_attach, instances=instances)
    dma, audio = instances['loadermemorydma'], instances['audioserialcontrol']
    dma.audio_endpoint, audio.dma_owner = audio, dma
    if still_pipeline_profile not in (None, *STILL_PIPELINE_PROFILES):
        raise ValueError('Unknown declared still pipeline profile')
    if still_pipeline_profile is not None:
        from .still_processing_devices import PROFILE as STILL_PROCESSING_PROFILE
        board.attach(StillProcessingControl(board,profile=STILL_PROCESSING_PROFILE))
        board.attach(JPEGEncodeControl(board,profile=JPEG_IDLE_RESET_PROFILE))
        if still_pipeline_profile == STILL_PIPELINE_RESIZE_PROFILE:
            board.attach(StillResizeControl(board,profile=STILL_RESIZE_PROFILE))
    if ui_conversion_profile is not None:
        board.attach(DeclaredUIConversion(board, profile=ui_conversion_profile))
    if fff63_profile is not None:
        board.attach(FFF63Engine(board, profile=fff63_profile))
    if mode_devices_profile is not None:
        board.attach(FFF48Engine(board, profile=mode_devices_profile))
    if settings not in (None,'default','owner'):
        raise ValueError('Settings must be default or owner')
    if settings=='owner' and (storage_profile!='captured' or user_journals not in (None,'restored') or mcu_profile not in (None,'captured-backup')):
        raise ValueError('Owner settings require captured storage and compatible journals/MCU inputs')
    return board


def restart_with_sd_media(board):
    """Construct a cold board retaining only SD sectors and external card levels."""
    cards=[device for device in board.devices if isinstance(device,SDCardController)]
    if len(cards)!=1:
        raise ValueError('Media-only cold restart requires exactly one supported SD controller')
    source=cards[0]
    options=dict(board._portable_factory_options)
    if options.get('firmware_profile') is not None:
        from x70_native.portable_checkpoint import _resolve_factory_profile
        options['firmware_profile']=_resolve_factory_profile(options['firmware_profile'])
    options['sd_card_present']=source._present()
    replacement=make_board(**options)
    target=next(device for device in replacement.devices if isinstance(device,SDCardController))
    target.card=source.card.retained_copy()
    target.image=source.image
    target.controls.set_gpio_input_pin(12,6,int(bool(source.controls.gpio_input_pin(12,6))),
                                      source='declared-sd-media-cold-restart')
    replacement.record('SD media-only cold restart',retained='SD base and guest sector overlays only',
                       excluded='RAM, CPU, pending events, NAND/settings mutations and failure controls')
    return replacement


# Keep optional extensions below the original factory callback definitions:
# their source locations are part of portable checkpoint callback identity.
from .still_processing_devices import StillProcessingControl
from .jpeg_encode import JPEGEncodeControl, IDLE_RESET_PROFILE as JPEG_IDLE_RESET_PROFILE
STILL_PIPELINE_PROFILE = 'declared-still-pipeline-mode0-jpeg-unmeasured'
from .still_resize_devices import StillResizeControl, PROFILE as STILL_RESIZE_PROFILE
STILL_PIPELINE_RESIZE_PROFILE = 'declared-still-pipeline-mode0-jpeg-resize-unmeasured'
STILL_PIPELINE_PROFILES = (STILL_PIPELINE_PROFILE, STILL_PIPELINE_RESIZE_PROFILE)


class BootSession:
    """One stock-boot board, advanced in bounded runs and saved checkpoints."""
    def __init__(self, output, *, entry='rom',quantum=1000,storage_profile='erased',graphics_profile=None,
                 backdrop_profile=None,display_frame_ticks=None,copy_profile=None,clear_profile=None,irq_tie_profile=None,
                 mcu_profile=None,user_journals=None,phase_profile='bit16',spi0_input_profile=None,
                 control_bus_profile=None,secondary_bus_profile=None,input_processing_profile=None,input_phase_profile=None,
                 input_event_profile=None,input_event_ticks=400000,counter_input_profile=None,
                 lens_response_profile=None,lens_response_ticks=400000,lens_continuation_profile=None,
                 sd_card_profile=None,sd_card_present=False,sd_card_transfer_ticks=100,sd_card_sectors=0x800000,
                 wifi_sdio_profile=None,wifi_sdio_transfer_ticks=100,
                 auxiliary_control_profile=None,auxiliary_control_ticks=2,
                 producer_stop_profile=None,producer_stop_ticks=2,cache_instructions=True,
                 producer_data_profile=None,producer_data_fixture=None,producer_data_ticks=400000,producer_ack_ticks=2,
                 image_work_stop_profile=None,image_work_stop_ticks=2,
                 image_work_data_profile=None,image_work_data_ticks=400000,
                 histogram_profile=None,histogram_data_ticks=400000,
                 movrsz_profile=None,movrsz_ticks=400000,face_stop_profile=None,
                 af_extraction_profile=None,af_extraction_ticks=400000,af_extraction_stop_ticks=2,
                 shutter_irq_profile=None,afs_frame_source_profile=None,afs_frame_source_ticks=400000,
                 still_pipeline_profile=None,settings=None,firmware_profile=None,instructions_per_tick=1,
                 ui_conversion_profile=None):
        self.ui_conversion_profile = UIConversionProfile.parse(ui_conversion_profile)
        self.output=output
        output.mkdir(parents=True,exist_ok=False)
        self.entry,self.quantum,self.storage_profile=entry,quantum,storage_profile
        self.graphics_profile=graphics_profile
        self.backdrop_profile=backdrop_profile
        self.display_frame_ticks=display_frame_ticks
        self.copy_profile,self.cache_instructions=copy_profile,cache_instructions
        self.clear_profile=clear_profile
        self.irq_tie_profile=irq_tie_profile
        self.mcu_profile,self.user_journals=mcu_profile,user_journals
        self.settings=settings
        self.firmware_profile=firmware_profile
        self.instructions_per_tick=instructions_per_tick
        self.phase_profile=phase_profile
        self.spi0_input_profile=spi0_input_profile
        self.control_bus_profile=control_bus_profile
        self.secondary_bus_profile=secondary_bus_profile
        self.input_processing_profile=input_processing_profile
        self.input_phase_profile=input_phase_profile
        self.input_event_profile,self.input_event_ticks=input_event_profile,input_event_ticks
        self.counter_input_profile=counter_input_profile
        self.lens_response_profile,self.lens_response_ticks=lens_response_profile,lens_response_ticks
        self.lens_continuation_profile=lens_continuation_profile
        self.sd_card_profile,self.sd_card_present=sd_card_profile,sd_card_present
        self.sd_card_transfer_ticks,self.sd_card_sectors=sd_card_transfer_ticks,sd_card_sectors
        self.wifi_sdio_profile,self.wifi_sdio_transfer_ticks=wifi_sdio_profile,wifi_sdio_transfer_ticks
        self.auxiliary_control_profile,self.auxiliary_control_ticks=auxiliary_control_profile,auxiliary_control_ticks
        self.producer_stop_profile,self.producer_stop_ticks=producer_stop_profile,producer_stop_ticks
        self.producer_data_profile,self.producer_data_fixture=producer_data_profile,producer_data_fixture
        self.producer_data_ticks,self.producer_ack_ticks=producer_data_ticks,producer_ack_ticks
        self.image_work_stop_profile,self.image_work_stop_ticks=image_work_stop_profile,image_work_stop_ticks
        self.image_work_data_profile,self.image_work_data_ticks=image_work_data_profile,image_work_data_ticks
        self.histogram_profile,self.histogram_data_ticks=histogram_profile,histogram_data_ticks
        self.movrsz_profile,self.movrsz_ticks=movrsz_profile,movrsz_ticks
        self.face_stop_profile=face_stop_profile
        self.af_extraction_profile=af_extraction_profile
        self.af_extraction_ticks,self.af_extraction_stop_ticks=af_extraction_ticks,af_extraction_stop_ticks
        self.shutter_irq_profile=shutter_irq_profile
        self.afs_frame_source_profile,self.afs_frame_source_ticks=afs_frame_source_profile,afs_frame_source_ticks
        self.still_pipeline_profile=still_pipeline_profile
        self.board=make_board(entry,quantum,storage_profile,graphics_profile,
                              backdrop_profile=backdrop_profile,
                              display_frame_ticks=display_frame_ticks,copy_profile=copy_profile,
                              clear_profile=clear_profile,
                              irq_tie_profile=irq_tie_profile,
                              mcu_profile=mcu_profile,user_journals=user_journals,settings=settings,
                              phase_profile=phase_profile,
                              spi0_input_profile=spi0_input_profile,
                              control_bus_profile=control_bus_profile,
                              secondary_bus_profile=secondary_bus_profile,
                              input_processing_profile=input_processing_profile,
                              input_phase_profile=input_phase_profile,
                              input_event_profile=input_event_profile,input_event_ticks=input_event_ticks,
                              counter_input_profile=counter_input_profile,
                              lens_response_profile=lens_response_profile,lens_response_ticks=lens_response_ticks,
                              lens_continuation_profile=lens_continuation_profile,
                              sd_card_profile=sd_card_profile,sd_card_present=sd_card_present,
                              sd_card_transfer_ticks=sd_card_transfer_ticks,sd_card_sectors=sd_card_sectors,
                              wifi_sdio_profile=wifi_sdio_profile,wifi_sdio_transfer_ticks=wifi_sdio_transfer_ticks,
                              auxiliary_control_profile=auxiliary_control_profile,auxiliary_control_ticks=auxiliary_control_ticks,
                              producer_stop_profile=producer_stop_profile,producer_stop_ticks=producer_stop_ticks,
                              producer_data_profile=producer_data_profile,producer_data_fixture=producer_data_fixture,
                              producer_data_ticks=producer_data_ticks,producer_ack_ticks=producer_ack_ticks,
                              image_work_stop_profile=image_work_stop_profile,image_work_stop_ticks=image_work_stop_ticks,
                              image_work_data_profile=image_work_data_profile,image_work_data_ticks=image_work_data_ticks,
                              histogram_profile=histogram_profile,histogram_data_ticks=histogram_data_ticks,
                              movrsz_profile=movrsz_profile,movrsz_ticks=movrsz_ticks,
                              face_stop_profile=face_stop_profile,
                              af_extraction_profile=af_extraction_profile,
                              af_extraction_ticks=af_extraction_ticks,af_extraction_stop_ticks=af_extraction_stop_ticks,
                              shutter_irq_profile=shutter_irq_profile,
                              afs_frame_source_profile=afs_frame_source_profile,
                              afs_frame_source_ticks=afs_frame_source_ticks,
                              still_pipeline_profile=still_pipeline_profile,
                              firmware_profile=firmware_profile,instructions_per_tick=instructions_per_tick,
                              ui_conversion_profile=self.ui_conversion_profile,
                              cache_instructions=cache_instructions)
        self.state=None
        self.checkpoint=None
        self.raw=self.png=b''
        self.frame_instance=None

    def prepare_checkpoint(self, after):
        """Capture a natural, event-drained boundary before a known blocker."""
        if isinstance(after,bool) or not isinstance(after,int) or after<1:
            raise ValueError('Checkpoint prefix must be a positive instruction count')
        if after<self.board.instructions:
            raise ValueError('Checkpoint threshold must not precede the current instruction count')
        started=perf_counter()
        deadline,threshold=after+100_000,after
        refusals=[]
        while True:
            self.board.run(deadline-self.board.instructions,pause_after=threshold)
            try:
                self.checkpoint=capture(self.board)
                break
            except CheckpointError as error:
                # Only wait for transactions that can drain. Identity errors,
                # failed execution and incomplete boundaries remain explicit.
                transient=(str(error)=='Pending scheduled closures are not supported; choose an event-drained boundary'
                           or str(error).endswith(' still has an in-flight operation'))
                if (not transient or self.board.failure or not self.board._checkpoint_boundary
                        or self.board.instructions>=deadline):
                    state=self.run({'limit':1},_execute=False)
                    raise CheckpointError(f"{error}; boot state saved in {state['artifact_directory']}") from error
                refusals.append({'instructions':self.board.instructions,'reason':str(error)})
                threshold=self.board.instructions+self.board.quantum
        metadata={**self.checkpoint.metadata,'prefix_and_capture_seconds':perf_counter()-started,
                  'capture_refusals':refusals}
        (self.output/('resume-'+uuid4().hex[:12]+'.json')).write_text(json.dumps(metadata,indent=2)+'\n')
        return metadata

    def restore_checkpoint(self):
        if self.checkpoint is None:
            raise ValueError('No resumable checkpoint in this process')
        restored=self.checkpoint.restore(self.board)
        self.raw=self.png=b''
        self.frame_instance=None
        self.state=None
        return restored

    def run(self, options, *, _execute=True):
        if set(options)-{'limit','reset','restore'}:
            raise ValueError('Unknown boot option')
        limit=options.get('limit',1_000_000)
        if isinstance(limit,bool) or not isinstance(limit,int) or not 1<=limit<=100_000_000:
            raise ValueError('Instruction limit must be 1..100000000')
        reset=options.get('reset',False)
        if not isinstance(reset,bool):
            raise ValueError('Reset must be a boolean')
        restore=options.get('restore',False)
        if not isinstance(restore,bool) or reset and restore:
            raise ValueError('Restore must be a boolean and cannot accompany reset')
        if restore:
            self.restore_checkpoint()
        if reset:
            self.board=make_board(self.entry,self.quantum,self.storage_profile,self.graphics_profile,
                                  backdrop_profile=self.backdrop_profile,
                                  display_frame_ticks=self.display_frame_ticks,copy_profile=self.copy_profile,
                                  clear_profile=self.clear_profile,
                                  irq_tie_profile=self.irq_tie_profile,
                                  mcu_profile=self.mcu_profile,user_journals=self.user_journals,settings=self.settings,
                                  phase_profile=self.phase_profile,
                                  spi0_input_profile=self.spi0_input_profile,
                                  control_bus_profile=self.control_bus_profile,
                                  secondary_bus_profile=self.secondary_bus_profile,
                                  input_processing_profile=self.input_processing_profile,
                                  input_phase_profile=self.input_phase_profile,
                                  input_event_profile=self.input_event_profile,input_event_ticks=self.input_event_ticks,
                                  counter_input_profile=self.counter_input_profile,
                                  lens_response_profile=self.lens_response_profile,lens_response_ticks=self.lens_response_ticks,
                                  lens_continuation_profile=self.lens_continuation_profile,
                                  sd_card_profile=self.sd_card_profile,sd_card_present=self.sd_card_present,
                                  sd_card_transfer_ticks=self.sd_card_transfer_ticks,sd_card_sectors=self.sd_card_sectors,
                                  wifi_sdio_profile=self.wifi_sdio_profile,
                                  wifi_sdio_transfer_ticks=self.wifi_sdio_transfer_ticks,
                                  auxiliary_control_profile=self.auxiliary_control_profile,auxiliary_control_ticks=self.auxiliary_control_ticks,
                                  producer_stop_profile=self.producer_stop_profile,producer_stop_ticks=self.producer_stop_ticks,
                                  producer_data_profile=self.producer_data_profile,producer_data_fixture=self.producer_data_fixture,
                                  producer_data_ticks=self.producer_data_ticks,producer_ack_ticks=self.producer_ack_ticks,
                                  image_work_stop_profile=self.image_work_stop_profile,image_work_stop_ticks=self.image_work_stop_ticks,
                                  image_work_data_profile=self.image_work_data_profile,image_work_data_ticks=self.image_work_data_ticks,
                                  histogram_profile=self.histogram_profile,histogram_data_ticks=self.histogram_data_ticks,
                                  movrsz_profile=self.movrsz_profile,movrsz_ticks=self.movrsz_ticks,
                                  face_stop_profile=self.face_stop_profile,
                                  af_extraction_profile=self.af_extraction_profile,
                                  af_extraction_ticks=self.af_extraction_ticks,
                                  af_extraction_stop_ticks=self.af_extraction_stop_ticks,
                                  shutter_irq_profile=self.shutter_irq_profile,
                                  afs_frame_source_profile=self.afs_frame_source_profile,
                                  afs_frame_source_ticks=self.afs_frame_source_ticks,
                                  still_pipeline_profile=self.still_pipeline_profile,
                                  firmware_profile=self.firmware_profile,instructions_per_tick=self.instructions_per_tick,
                                  ui_conversion_profile=self.ui_conversion_profile,
                                  cache_instructions=self.cache_instructions)
            self.raw=self.png=b''
            self.frame_instance=None
            self.checkpoint=None
        directory=self.output/('checkpoint-'+uuid4().hex[:12])
        directory.mkdir()
        started=perf_counter()
        previous_instructions=self.board.instructions
        try:
            if _execute:
                self.board.run(limit)
        finally:
            frames={}
            for instance in (0,1):
                try:
                    frame=self.board.display.frame_boundary(instance)
                except UnsupportedScanout:
                    frame=None
                if frame is not None:
                    frames[instance]=frame
                    (directory/f'frame-{instance}.bin').write_bytes(frame.raw)
                    (directory/f'frame-{instance}.png').write_bytes(frame.png)
            if frames:
                self.frame_instance=0 if 0 in frames else 1
                frame=frames[self.frame_instance]
                self.raw,self.png=frame.raw,frame.png
            self.state=self.board.snapshot()
            execution_seconds=perf_counter()-started
            executed=self.board.instructions-previous_instructions
            self.state['performance']={'execution_and_frame_sampling_seconds':execution_seconds,
                'instructions_this_run':executed,
                'instructions_per_second':executed/execution_seconds if execution_seconds else None}
            self.state['resume_checkpoint']={'present':self.checkpoint is not None}
            if self.checkpoint is not None:
                self.state['resume_checkpoint'].update(metadata=self.checkpoint.metadata,
                                                       last_restore=self.checkpoint.last_restore)
            self.state['storage_profile']=self.board.storage_profile
            self.state['devices']=[d.snapshot() for d in self.board.devices if hasattr(d,'snapshot')]
            selected=self.frame_instance
            self.state['display']={'selected_instance':selected,
                'frame_count':0 if selected is None else self.board.display.frames[selected],
                'last_frame':None if selected is None else self.board.display.frame_state[selected],
                'instances':[{'instance':i,'frame_count':self.board.display.frames[i],
                              'last_frame':self.board.display.frame_state[i],
                              'render_error':self.board.display.render_errors[i]} for i in (0,1)]}
            self.state['artifact_directory']=str(directory.resolve())
            (directory/'state.json').write_text(json.dumps(self.state,indent=2)+'\n')
        return self.state


def main(*, session_factory=None):
    if session_factory is None:
        from x70_native.boot import NativeBootSession
        session_factory=NativeBootSession
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--entry',choices=('rom','loader'),default='rom')
    parser.add_argument('--firmware-profile',type=Path)
    parser.add_argument('--instructions-per-tick',type=int,default=24)
    parser.add_argument('--limit',type=int,default=1_000_000)
    parser.add_argument('--quantum',type=int,default=1000)
    parser.add_argument('--output',type=Path)
    parser.add_argument('--serve',action='store_true',help='Open the stock-boot control/status viewer')
    parser.add_argument('--open',action='store_true',help='Open the loopback viewer in a browser')
    parser.add_argument('--port',type=int,default=0)
    parser.add_argument('--bind',default='127.0.0.1',help='Loopback or Tailscale IPv4 listener address')
    parser.add_argument('--hostname',help='Accepted hostname and hostname to print in the viewer URL')
    parser.add_argument('--settings',choices=('default','owner'),help='Opt-in newest owner persistent preferences')
    parser.add_argument('--storage-profile',choices=('captured','erased'),default='erased')
    parser.add_argument('--mcu-profile',choices=('captured-backup',),
                        help='Explicit saved RAM backup packet as inferred MCU input, not a captured SPI response')
    parser.add_argument('--user-journals',choices=('restored','main-read'),
                        help='Explicit captured main/custom user journals with synthetic NAND placement')
    parser.add_argument('--spi0-input-profile',choices=('absent-external-flash-fixed-low-miso',),
                        help='Opt-in virtual absent external-flash input, not a captured SPI reply')
    parser.add_argument('--control-bus-profile',choices=CONTROL_BUS_PROFILES,
                        help='Opt-in bounded transport/input fixture with real IRQ dispatch')
    parser.add_argument('--secondary-bus-profile',choices=SECONDARY_BUS_PROFILES,
                        help='Explicit virtual disconnected USB-charge register inputs')
    parser.add_argument('--input-processing-profile',choices=INPUT_PROCESSING_PROFILES,
                        help='Opt-in bounded write-only sensor command transmit completion; no image or sensor readback')
    parser.add_argument('--input-phase-profile',choices=INPUT_PHASE_PROFILES,
                        help='Opt-in inferred inactive phase quiescence acknowledgment; no frame or interrupt')
    parser.add_argument('--input-event-profile',choices=INPUT_EVENT_PROFILES,
                        help='Opt-in unmeasured external input0 edges through actual source1/IRQ29 and source0/IRQ27 routes; no pixels or DMA')
    parser.add_argument('--input-event-ticks',type=int,default=400000,
                        help='Positive inferred delay to the single external input edge; independent of display timing')
    parser.add_argument('--counter-input-profile',choices=PulseOutputs.COUNTER_INPUT_PROFILES,
                        help='Opt-in stationary/disconnected fixed-low position lines; no motion, edges or capture/reload')
    parser.add_argument('--lens-response-profile',choices=LENS_RESPONSE_PROFILES,
                        help='Declared initial focus/iris response experiment; homing and physical timing unmeasured')
    parser.add_argument('--lens-response-ticks',type=int,default=400000)
    parser.add_argument('--lens-continuation-profile',choices=LENS_CONTINUATION_PROFILES,
                        help='Opt-in bounded post-completion lens wire configuration; no new motion or response')
    parser.add_argument('--sd-card-profile',choices=SD_CARD_PROFILES,
                        help='Declared virtual SD0 command/sector model')
    parser.add_argument('--sd-card-present',action='store_true',
                        help='Declare active-low CARD input at boot; requires a card profile')
    parser.add_argument('--sd-card-transfer-ticks',type=int,default=100)
    parser.add_argument('--sd-card-sectors',type=int,default=0x800000)
    parser.add_argument('--wifi-sdio-profile',choices=WIFI_SDIO_PROFILES,
                        help='Declared host1 SDIO enumeration or ROM firmware download; network traffic remains strict')
    parser.add_argument('--wifi-sdio-transfer-ticks',type=int,default=100)
    parser.add_argument('--af-extraction-profile',choices=AF_EXTRACTION_PROFILES,
                        help='Declared raw PDAF extraction arithmetic; stock firmware decides focus')
    parser.add_argument('--af-extraction-ticks',type=int,default=400000)
    parser.add_argument('--af-extraction-stop-ticks',type=int,default=2)
    parser.add_argument('--shutter-irq-profile',choices=(SHUTTER_IRQ_PROFILE,),
                        help='Declared falling GPIO shutter edges through stock IRQ10/IRQ11 registrations')
    parser.add_argument('--afs-frame-source-profile',choices=AFS_FRAME_SOURCE_PROFILES,
                        help='Attach one explicit post-S1 sensor source0 opportunity; disabled until scenario arm')
    parser.add_argument('--afs-frame-source-ticks',type=int,default=400000)
    parser.add_argument('--still-pipeline-profile',choices=STILL_PIPELINE_PROFILES,
                        help='Explicit mode0 processing and JPEG devices; scene completions require separate admission')
    parser.add_argument('--ui-conversion-profile',type=UIConversionProfile.parse,
                        choices=tuple(UIConversionProfile),
                        help='Explicit declared UI CPU fallback; silicon behavior and timing unmeasured')
    parser.add_argument('--auxiliary-control-profile',choices=AUXILIARY_CONTROL_PROFILES,
                        help='Opt-in exact auxiliary3→2→3 acknowledgment; hardware meaning/delay unmeasured, no pixels')
    parser.add_argument('--auxiliary-control-ticks',type=int,default=2,
                        help='Positive virtual delay for the auxiliary control acknowledgment')
    parser.add_argument('--producer-stop-profile',choices=PRODUCER_STOP_PROFILES,
                        help='Opt-in recovered idle stop handshakes only; unmeasured timing, no capture/DMA data or IRQ')
    parser.add_argument('--producer-stop-ticks',type=int,default=2,
                        help='Positive virtual delay for the producer stop acknowledgment')
    parser.add_argument('--producer-data-profile',choices=PRODUCER_DATA_PROFILES,
                        help='Explicit one-shot foreign captured statistics consumer experiment')
    parser.add_argument('--producer-data-fixture',help='Verified recorded-statistics fixture directory; requires the explicit data profile')
    parser.add_argument('--producer-data-ticks',type=int,default=400000)
    parser.add_argument('--producer-ack-ticks',type=int,default=2)
    parser.add_argument('--image-work-stop-profile',choices=IMAGE_WORK_STOP_PROFILES,
                        help='Opt-in initial image-bank stop acknowledgment; hardware response/delay unmeasured')
    parser.add_argument('--image-work-stop-ticks',type=int,default=2)
    parser.add_argument('--image-work-data-profile',choices=IMAGE_WORK_DATA_PROFILES,
                        help='Declared virtual post-ISP scene; clock, pitch and latching are unmeasured')
    parser.add_argument('--image-work-data-ticks',type=int,default=400000)
    parser.add_argument('--histogram-profile',choices=HISTOGRAM_PROFILES,
                        help='Declared virtual post-ISP histogram; sampling, clock and latching are unmeasured')
    parser.add_argument('--histogram-data-ticks',type=int,default=400000)
    parser.add_argument('--movrsz-profile',choices=(FujiDecompressor.MOVRSZ_PROFILE,),
                        help='Declared source-derived pyramid resize; pixel filter and timing unmeasured')
    parser.add_argument('--movrsz-ticks',type=int,default=400000)
    parser.add_argument('--face-stop-profile',choices=FACE_STOP_PROFILES,
                        help='Declared cancellation of the armed face request with idle DMA polls')
    parser.add_argument('--graphics-profile',choices=DisplayScanout.GRAPHICS_PROFILES,
                        help='Explicit inferred interpretation for the actual mode1 graphics RAM')
    parser.add_argument('--backdrop-profile',choices=DisplayScanout.BACKDROP_PROFILES,
                        help='Explicit unmeasured canvas behind transparent graphics without main video')
    parser.add_argument('--display-frame-ticks',type=int,
                        help='Explicit inferred virtual display frame period; omitted disables recurring phases')
    parser.add_argument('--phase-profile',choices=('bit16','configured-clocked'),default='bit16',
                        help='Opt-in inferred phase cadence with complete timing configuration and acknowledged clock')
    parser.add_argument('--copy-profile',choices=(GraphicsAccelerator.COPY_PROFILE,),
                        help='Explicit software-derived GPU copy interpretation, not hardware validated')
    parser.add_argument('--clear-profile',choices=(GraphicsAccelerator.CLEAR_PROFILE, GraphicsAccelerator.UI_PROFILE),
                        help='Explicit bounded stock-VG clear or declared UI primitive interpretation')
    parser.add_argument('--irq-tie-profile',choices=('kernel-active-scan-order',),
                        help='Explicit inferred equal-priority IRQ selection; physical arbitration unvalidated')
    parser.add_argument('--checkpoint-after',type=int,
                        help='Retain an in-memory checkpoint at a natural boundary after this ROM prefix')
    args=parser.parse_args()
    if not 1<=args.limit<=100_000_000 or not 1<=args.quantum<=10000:
        parser.error('Limit 1..100000000 and quantum 1..10000 required')
    if args.open and not args.serve:
        parser.error('--open requires --serve')
    if args.display_frame_ticks is not None and args.display_frame_ticks<4:
        parser.error('Virtual display frame period must be >=4')
    if args.input_event_ticks<1:
        parser.error('Input event delay must be positive')
    if args.lens_response_ticks<1:
        parser.error('Positive virtual lens response delay required')
    if args.auxiliary_control_ticks<1:
        parser.error('Auxiliary control delay must be positive')
    if args.producer_stop_ticks<1:
        parser.error('Producer stop delay must be positive')
    if min(args.producer_data_ticks,args.producer_ack_ticks)<1:
        parser.error('Positive producer data and acknowledgment delays required')
    if args.image_work_stop_ticks<1:
        parser.error('Image-bank stop delay must be positive')
    if args.image_work_data_ticks<1:
        parser.error('Virtual image delay must be positive')
    if args.histogram_data_ticks<1:
        parser.error('Virtual histogram delay must be positive')
    if args.movrsz_ticks<1:
        parser.error('Virtual pyramid resize delay must be positive')
    if args.wifi_sdio_transfer_ticks<1:
        parser.error('Virtual WiFi SDIO transfer delay must be positive')
    if min(args.af_extraction_ticks,args.af_extraction_stop_ticks)<1:
        parser.error('Positive AF extraction and stop delays required')
    if not 1<=args.afs_frame_source_ticks<1<<64:
        parser.error('Positive bounded AF-S frame source delay required')
    if (args.producer_data_profile is None)!=(args.producer_data_fixture is None):
        parser.error('Producer data profile and fixture must be selected together')
    if args.checkpoint_after is not None and not 1<=args.checkpoint_after<args.limit-100_000:
        parser.error('Checkpoint prefix must leave at least100000 instructions before --limit')
    output=args.output or ROOT/'.tools/emulator-runs'/(
        datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid4().hex[:8])
    session=session_factory(output,entry=args.entry,quantum=args.quantum,
                        firmware_profile=resolve('xm1-150',profile_file=str(args.firmware_profile)) if args.firmware_profile else None,
                        instructions_per_tick=args.instructions_per_tick,
                        ui_conversion_profile=args.ui_conversion_profile,
                        storage_profile=args.storage_profile,graphics_profile=args.graphics_profile,
                        backdrop_profile=args.backdrop_profile,
                        display_frame_ticks=args.display_frame_ticks,copy_profile=args.copy_profile,
                        clear_profile=args.clear_profile,
                        irq_tie_profile=args.irq_tie_profile,
                        mcu_profile=args.mcu_profile,user_journals=args.user_journals,settings=args.settings,
                        phase_profile=args.phase_profile,
                        spi0_input_profile=args.spi0_input_profile,
                        control_bus_profile=args.control_bus_profile,
                        secondary_bus_profile=args.secondary_bus_profile,
                        input_processing_profile=args.input_processing_profile,
                        input_phase_profile=args.input_phase_profile,
                        input_event_profile=args.input_event_profile,input_event_ticks=args.input_event_ticks,
                        counter_input_profile=args.counter_input_profile,
                        lens_response_profile=args.lens_response_profile,lens_response_ticks=args.lens_response_ticks,
                        lens_continuation_profile=args.lens_continuation_profile,
                        sd_card_profile=args.sd_card_profile,sd_card_present=args.sd_card_present,
                        sd_card_transfer_ticks=args.sd_card_transfer_ticks,sd_card_sectors=args.sd_card_sectors,
                        wifi_sdio_profile=args.wifi_sdio_profile,
                        wifi_sdio_transfer_ticks=args.wifi_sdio_transfer_ticks,
                        auxiliary_control_profile=args.auxiliary_control_profile,auxiliary_control_ticks=args.auxiliary_control_ticks,
                        producer_stop_profile=args.producer_stop_profile,producer_stop_ticks=args.producer_stop_ticks,
                        producer_data_profile=args.producer_data_profile,producer_data_fixture=args.producer_data_fixture,
                        producer_data_ticks=args.producer_data_ticks,producer_ack_ticks=args.producer_ack_ticks,
                        image_work_stop_profile=args.image_work_stop_profile,image_work_stop_ticks=args.image_work_stop_ticks,
                        image_work_data_profile=args.image_work_data_profile,image_work_data_ticks=args.image_work_data_ticks,
                        histogram_profile=args.histogram_profile,histogram_data_ticks=args.histogram_data_ticks,
                        movrsz_profile=args.movrsz_profile,movrsz_ticks=args.movrsz_ticks,
                        face_stop_profile=args.face_stop_profile,
                        af_extraction_profile=args.af_extraction_profile,
                        af_extraction_ticks=args.af_extraction_ticks,
                        af_extraction_stop_ticks=args.af_extraction_stop_ticks,
                        shutter_irq_profile=args.shutter_irq_profile,
                        afs_frame_source_profile=args.afs_frame_source_profile,
                        afs_frame_source_ticks=args.afs_frame_source_ticks,
                        still_pipeline_profile=args.still_pipeline_profile,
                        cache_instructions=True)
    if args.checkpoint_after is not None:
        metadata=session.prepare_checkpoint(args.checkpoint_after)
        print(f"Resumable prefix: {metadata['instructions']:,} instructions",flush=True)
    state=session.run({'limit':max(1,args.limit-session.board.instructions)})
    print('Saved: '+state['artifact_directory'],flush=True)
    print(f"Executed {state['instructions']:,} stock instructions",flush=True)
    if state['failure']:
        print('Stopped: '+json.dumps(state['failure']),flush=True)
    else:
        print('Instruction limit reached; boot completion is not established.',flush=True)
    if args.serve:
        from .server import make_server
        server=make_server(session,args.port,page='boot.html',bind=args.bind,hostname=args.hostname)
        url=f'http://{args.hostname or args.bind}:{server.server_port}/'
        print('Stock-boot viewer: '+url,flush=True)
        if args.open:
            webbrowser.open(url)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()


if __name__=='__main__':
    main()
