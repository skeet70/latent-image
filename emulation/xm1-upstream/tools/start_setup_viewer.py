#!/usr/bin/env python3
"""Boot X-M1 1.50 from its captured ROM and serve guest-produced display planes."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group()
    source.add_argument('--snapshot', type=Path)
    source.add_argument('--profile', type=Path, default=ROOT / 'firmware/x-m1/1.50/emulator-rec.json')
    source.add_argument('--firmware', type=Path, help='Stock or modified X-M1 1.50-compatible DAT')
    parser.add_argument('--firmware-name', help='Display filename for the selected DAT')
    parser.add_argument('--save-snapshot', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--port', type=int, default=0)
    args = parser.parse_args()
    if args.bind != '127.0.0.1':
        parser.error('The X-M1 viewer binds only to127.0.0.1')
    if not 0 <= args.port <= 65535:
        parser.error('Port must be0..65535')

    profile, identity = None, None
    if args.firmware:
        from tools.firmware_registry import prepare_xm1_viewer
        with args.firmware.open('rb') as handle:
            source_bytes = handle.read(64 * 1024 * 1024 + 1)
        profile, identity = prepare_xm1_viewer(
            source_bytes, args.firmware_name or args.firmware.name)
        os.environ['XM1_VIEWER_INITIAL_FIRMWARE'] = profile.firmware_id
    elif args.snapshot:
        manifest = json.loads((args.snapshot / 'manifest.json').read_text())
        selected = manifest.get('factory_options', {}).get('firmware_profile')
        if selected:
            os.environ['XM1_VIEWER_INITIAL_FIRMWARE'] = selected['firmware_id']
    else:
        document = json.loads(args.profile.read_text())
        if document.get('firmware_id'):
            os.environ['XM1_VIEWER_INITIAL_FIRMWARE'] = document['firmware_id']

    from emulator.boot import make_board
    from emulator.server import make_menu_server
    from tools.continue_rec_scanout import console_summary, load_profile
    from tools.probe_setup_menus import MenuSession

    output = args.output or ROOT / '.tools/xm1-viewer' / uuid4().hex[:12]
    if args.snapshot:
        from x70_native.portable_checkpoint import load
        board = load(args.snapshot)
        options = board._portable_factory_options
        session = MenuSession(board, output, options=options)
        session.save_checkpoint()
        session.sample()
    else:
        options = load_profile(args.profile, make_board, firmware_profile=profile)
        if identity is None:
            from tools import firmware_registry as registry
            entry = registry.load(options['firmware_profile'].firmware_id)
            identity = dict(filename=entry['source']['filename'], sha256=entry['source']['sha256'],
                            model=entry['model'], version=entry['version'],
                            modified=entry['source']['sha256'] != registry.load('xm1-150')['source']['sha256'])
        print(f'Executing X-M1 {identity["version"]}: {identity["filename"]} SHA-256 {identity["sha256"]}', flush=True)
        session = MenuSession.cold_boot(options, output, firmware=identity)
    if args.save_snapshot:
        print(json.dumps(session.save_snapshot(args.save_snapshot)), flush=True)
    print(json.dumps(console_summary(session.state)), flush=True)
    server = make_menu_server(session, args.port, page='boot.html', bind=args.bind)
    print(f'X-M1 firmware viewer: http://{args.bind}:{server.server_port}/', flush=True)
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == '__main__':
    main()
