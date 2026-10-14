"""Shared Rust execution CLI profiles, without a CPU-backend selector."""


def add_profile_arguments(parser):
    from emulator.loader_devices import FujiDecompressor
    from emulator.face_analysis_devices import STOP_PROFILES as FACE_STOP_PROFILES
    from emulator.af_extraction import PROFILES as AF_EXTRACTION_PROFILES
    from emulator.afs_frame_source import PROFILES as AFS_FRAME_SOURCE_PROFILES
    from emulator.boot_devices import SHUTTER_IRQ_PROFILE
    from emulator.boot import STILL_PIPELINE_PROFILES
    from emulator.graphics_devices import GraphicsAccelerator
    from emulator.scanout import DisplayScanout
    from emulator.control_bus import PROFILES as CONTROL_BUS_PROFILES
    from emulator.secondary_control_bus import PROFILES as SECONDARY_BUS_PROFILES
    from emulator.pulse_devices import PulseOutputs
    from emulator.lens_endpoint import RESPONSE_PROFILES as LENS_RESPONSE_PROFILES
    from emulator.input_auxiliary_devices import CONTROL_PROFILES as AUXILIARY_CONTROL_PROFILES
    from emulator.producer_devices import STOP_PROFILES as PRODUCER_STOP_PROFILES
    from emulator.producer_devices import DATA_PROFILES as PRODUCER_DATA_PROFILES
    from emulator.producer_devices import HISTOGRAM_PROFILES
    from emulator.image_work_devices import STOP_PROFILES as IMAGE_WORK_STOP_PROFILES
    from emulator.image_work_devices import DATA_PROFILES as IMAGE_WORK_DATA_PROFILES
    from emulator.input_processing_devices import (
        PROFILES as INPUT_PROCESSING_PROFILES,
        PHASE_PROFILES as INPUT_PHASE_PROFILES,
        EVENT_PROFILES as INPUT_EVENT_PROFILES,
    )
    parser.add_argument('--input-profile', choices=('setup', 'captured-normal'), default='setup')
    parser.add_argument('--execution', choices=('instructions', 'fast'), default=None,
                        help='native execution mode (default: fast, or X70_NATIVE_EXECUTION); '
                             'both are bit-exact; instructions is the historical fallback')
    parser.add_argument('--quantum', type=int, default=1000)
    parser.add_argument('--storage-profile', choices=('captured', 'erased'), default='captured')
    for name in ('graphics-profile', 'copy-profile', 'irq-tie-profile'):
        parser.add_argument('--'+name)
    parser.add_argument('--backdrop-profile',choices=('none',*DisplayScanout.BACKDROP_PROFILES))
    parser.add_argument('--clear-profile', choices=('none', GraphicsAccelerator.CLEAR_PROFILE,
                                                  GraphicsAccelerator.UI_PROFILE))
    parser.add_argument('--mcu-profile', choices=('none', 'captured-backup'))
    parser.add_argument('--user-journals', choices=('none', 'restored', 'main-read'))
    parser.add_argument('--display-frame-ticks', type=int)
    parser.add_argument('--phase-profile', choices=('bit16', 'configured-clocked'), default='bit16')
    parser.add_argument('--spi0-input-profile', choices=('none', 'absent-external-flash-fixed-low-miso'))
    parser.add_argument('--control-bus-profile', choices=('none', *CONTROL_BUS_PROFILES))
    parser.add_argument('--secondary-bus-profile', choices=('none', *SECONDARY_BUS_PROFILES))
    parser.add_argument('--input-processing-profile', choices=('none', *INPUT_PROCESSING_PROFILES))
    parser.add_argument('--input-phase-profile', choices=('none', *INPUT_PHASE_PROFILES))
    parser.add_argument('--input-event-profile', choices=('none', *INPUT_EVENT_PROFILES))
    parser.add_argument('--input-event-ticks', type=int, default=400000)
    parser.add_argument('--counter-input-profile', choices=('none', *PulseOutputs.COUNTER_INPUT_PROFILES))
    parser.add_argument('--lens-response-profile', choices=('none', *LENS_RESPONSE_PROFILES))
    parser.add_argument('--lens-response-ticks', type=int, default=400000)
    parser.add_argument('--auxiliary-control-profile', choices=('none', *AUXILIARY_CONTROL_PROFILES))
    parser.add_argument('--auxiliary-control-ticks', type=int, default=2)
    parser.add_argument('--producer-stop-profile', choices=('none', *PRODUCER_STOP_PROFILES))
    parser.add_argument('--producer-stop-ticks', type=int, default=2)
    parser.add_argument('--producer-data-profile', choices=PRODUCER_DATA_PROFILES)
    parser.add_argument('--producer-data-fixture')
    parser.add_argument('--producer-data-ticks', type=int, default=400000)
    parser.add_argument('--producer-ack-ticks', type=int, default=2)
    parser.add_argument('--image-work-stop-profile', choices=('none', *IMAGE_WORK_STOP_PROFILES))
    parser.add_argument('--image-work-stop-ticks', type=int, default=2)
    parser.add_argument('--image-work-data-profile', choices=('none', *IMAGE_WORK_DATA_PROFILES))
    parser.add_argument('--image-work-data-ticks', type=int, default=400000)
    parser.add_argument('--histogram-profile', choices=('none', *HISTOGRAM_PROFILES))
    parser.add_argument('--histogram-data-ticks', type=int, default=400000)
    parser.add_argument('--movrsz-profile', choices=('none', FujiDecompressor.MOVRSZ_PROFILE))
    parser.add_argument('--movrsz-ticks', type=int, default=400000)
    parser.add_argument('--face-stop-profile', choices=('none', *FACE_STOP_PROFILES))
    parser.add_argument('--af-extraction-profile', choices=('none', *AF_EXTRACTION_PROFILES))
    parser.add_argument('--af-extraction-ticks', type=int, default=400000)
    parser.add_argument('--af-extraction-stop-ticks', type=int, default=2)
    parser.add_argument('--shutter-irq-profile', choices=('none', SHUTTER_IRQ_PROFILE))
    parser.add_argument('--afs-frame-source-profile', choices=('none', *AFS_FRAME_SOURCE_PROFILES))
    parser.add_argument('--afs-frame-source-ticks', type=int, default=400000)
    parser.add_argument('--still-pipeline-profile', choices=('none', *STILL_PIPELINE_PROFILES))


def profile_options(args):
    for name, label in (
        ('quantum', 'quantum'), ('input_event_ticks', 'input event delay'),
        ('auxiliary_control_ticks', 'auxiliary control delay'),
        ('lens_response_ticks', 'lens response delay'),
        ('producer_stop_ticks', 'producer stop delay'),
        ('producer_data_ticks', 'producer data delay'),
        ('producer_ack_ticks', 'producer acknowledgment delay'),
        ('image_work_stop_ticks', 'image-bank stop delay'),
        ('image_work_data_ticks', 'virtual image delay'),
        ('histogram_data_ticks', 'virtual histogram delay'),
        ('movrsz_ticks', 'virtual pyramid resize delay'),
        ('af_extraction_ticks', 'AF extraction delay'),
        ('af_extraction_stop_ticks', 'AF extraction stop delay'),
        ('afs_frame_source_ticks', 'AF-S frame source delay'),
    ):
        if getattr(args, name) < 1:
            raise ValueError('Positive '+label+' required')
    if args.afs_frame_source_ticks>=1<<64:
        raise ValueError('Bounded AF-S frame source delay required')
    if (args.producer_data_profile is None)!=(args.producer_data_fixture is None):
        raise ValueError('Producer data profile and fixture must be selected together')
    if getattr(args, 'execution', None) is not None:
        # Native execution mode is engine state, not a board option: select
        # the process default used by boards built after parsing.
        from x70_native.board import set_default_execution
        set_default_execution(args.execution)
    options = {
        name: getattr(args, name) for name in (
            'quantum', 'storage_profile', 'graphics_profile', 'backdrop_profile', 'copy_profile',
            'clear_profile', 'irq_tie_profile', 'display_frame_ticks', 'phase_profile',
            'control_bus_profile', 'secondary_bus_profile', 'input_processing_profile',
            'input_phase_profile', 'input_event_profile', 'input_event_ticks',
            'counter_input_profile', 'auxiliary_control_profile', 'auxiliary_control_ticks',
            'lens_response_profile', 'lens_response_ticks',
            'producer_stop_profile', 'producer_stop_ticks',
            'producer_data_profile', 'producer_data_fixture', 'producer_data_ticks', 'producer_ack_ticks',
            'image_work_stop_profile', 'image_work_stop_ticks',
            'image_work_data_profile', 'image_work_data_ticks',
            'histogram_profile', 'histogram_data_ticks',
            'movrsz_profile', 'movrsz_ticks',
            'face_stop_profile',
            'af_extraction_profile', 'af_extraction_ticks', 'af_extraction_stop_ticks',
            'shutter_irq_profile',
            'afs_frame_source_profile', 'afs_frame_source_ticks',
            'still_pipeline_profile',
        ) if getattr(args, name) is not None
    }
    optional = dict(mcu_profile=None, user_journals=None, spi0_input_profile=None)
    if args.input_profile == 'captured-normal':
        optional.update(
            mcu_profile='captured-backup', user_journals='restored',
            spi0_input_profile='absent-external-flash-fixed-low-miso',
        )
    for name in optional:
        value = getattr(args, name)
        if value is not None:
            optional[name] = None if value == 'none' else value
    options.update(optional)
    return {key: None if value == 'none' else value for key, value in options.items()}
