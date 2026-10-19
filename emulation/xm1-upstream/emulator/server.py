"""Loopback or explicitly bound Tailscale viewer; no camera I/O."""
import json
import ipaddress
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from socketserver import TCPServer
from urllib.parse import unquote, urlsplit
from uuid import uuid4

from .runtime import DisplayEmulator


class LoopbackHTTPServer(HTTPServer):
    def server_bind(self):
        # HTTPServer ordinarily performs reverse DNS in server_bind. A local
        # offline viewer needs no lookup, which can stall on host DNS settings.
        TCPServer.server_bind(self)
        self.server_name = 'localhost'
        self.server_port = self.server_address[1]


class DisplaySession:
    def __init__(self, output: Path):
        self.output = output
        self.output.mkdir(parents=True, exist_ok=False)
        self.state = None
        self.raw = b''
        self.png = b''

    def run(self, options):
        allowed = {'scenario', 'route', 'lcd_selector', 'packing', 'matrix'}
        if set(options) - allowed:
            raise ValueError('Unknown run option')
        machine = DisplayEmulator()
        directory = self.output / ('run-' + uuid4().hex[:12])
        directory.mkdir()
        try:
            state, raw, png = machine.run_pattern(**options)
        except Exception as exc:
            (directory/'failure.json').write_text(json.dumps({'error': str(exc),
                'options': options, 'instructions': machine.instructions,
                'modeled_calls': machine.modeled_calls,
                'mmio_accesses': machine.mmio_accesses}, indent=2)+'\n')
            raise
        state['artifact_directory'] = str(directory.resolve())
        (directory/'state.json').write_text(json.dumps(state, indent=2)+'\n')
        (directory/'frame.bin').write_bytes(raw)
        (directory/'frame.png').write_bytes(png)
        self.state, self.raw, self.png = state, raw, png
        return state


def make_server(session, port=0, *, page='index.html',bind='127.0.0.1',hostname=None):
    address=ipaddress.ip_address(bind)
    if address.version!=4 or not (address.is_loopback or address in ipaddress.ip_network('100.64.0.0/10')):
        raise ValueError('Bind must be an IPv4 loopback or Tailscale address')
    names={bind}
    if address.is_loopback:
        names.add('localhost')
    if hostname:
        names.add(hostname.lower().rstrip('.'))
    page_path=Path(__file__).parent/'web'/page
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def _send(self, data, kind='application/json', status=200):
            self.send_response(status)
            self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Content-Security-Policy',
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' blob:; frame-ancestors 'none'")
            self.end_headers()
            self.wfile.write(data)

        def _json(self, value, status=200):
            self._send(json.dumps(value).encode(), status=status)

        def _allowed(self):
            port = self.server.server_address[1]
            hosts = tuple(f'{name}:{port}' for name in names)
            if self.headers.get('Host','').lower() not in hosts:
                self._json({'error': 'Configured Host required'}, 403)
                return False
            origin = self.headers.get('Origin')
            if origin and origin not in tuple('http://'+h for h in hosts):
                self._json({'error': 'Same-origin request required'}, 403)
                return False
            return True

        def do_GET(self):
            if not self._allowed():
                return
            path = urlsplit(self.path).path
            if path == '/':
                self._send(page_path.read_bytes(), 'text/html; charset=utf-8')
            elif path == '/api/state':
                self._json(session.state)
            elif path == '/frame.png':
                self._send(session.png, 'image/png')
            elif path == '/frame.bin':
                self._send(session.raw, 'application/octet-stream')
            else:
                self._json({'error': 'Not found'}, 404)

        def do_POST(self):
            if not self._allowed():
                return
            action = session.run if self.path == '/api/run' else (
                getattr(session, 'key', None) if self.path == '/api/key' else None)
            if not callable(action):
                self._json({'error': 'Not found'}, 404)
                return
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 4096:
                    raise ValueError('JSON body must be 1..4096 bytes')
                options = json.loads(self.rfile.read(length))
                if not isinstance(options, dict):
                    raise ValueError('Expected a JSON object')
                # HTTPServer dispatches on its serving thread. Native engines
                # stay on the thread that constructed/restored the session.
                self._json(action(options))
            except Exception as exc:
                self._json({'error': str(exc)}, 400)

    return LoopbackHTTPServer((bind, port), Handler)


def make_menu_server(session, port=0, *, page='index.html', bind='127.0.0.1', hostname=None):
    """Add host checkpoint controls without changing boot's server import.

    Portable identity pins boot.make_server through its imported code body.
    Keep that function stable while extending the diagnostic menu adapter.
    """
    server = make_server(session, port, page=page, bind=bind, hostname=hostname)
    base = server.RequestHandlerClass

    class MenuHandler(base):
        def _send(self, *args, **kwargs):
            try:
                return super()._send(*args, **kwargs)
            except (BrokenPipeError, ConnectionResetError):
                # A browser may cancel a queued refresh during a long run.
                # The owner-thread execution/result remains valid.
                return

        def do_POST(self):
            if self.path == '/api/firmware':
                if not self._allowed():
                    return
                action = getattr(session, 'replace_firmware', None)
                if not callable(action):
                    self._json({'error': 'Not found'}, 404)
                    return
                try:
                    if (self.headers.get_content_type() != 'application/octet-stream' or
                            self.headers.get('Transfer-Encoding') is not None or
                            len(self.headers.get_all('Content-Length', [])) != 1):
                        raise ValueError('Firmware requires raw application/octet-stream bytes and Content-Length')
                    length = int(self.headers['Content-Length'])
                    if not 0 < length <= 64 * 1024 * 1024:
                        raise ValueError('Firmware body must be 1..64 MiB')
                    encoded = self.headers.get('X-Firmware-Filename', 'firmware.DAT')
                    if len(encoded) > 4096:
                        raise ValueError('Firmware filename is too long')
                    filename = unquote(encoded, encoding='utf-8', errors='strict')
                    if not filename or any(ord(character) < 32 for character in filename):
                        raise ValueError('Firmware filename must be nonempty display text')
                    source = self.rfile.read(length)
                    if len(source) != length:
                        raise ValueError('Truncated firmware body')
                    self._json(action(source, filename))
                except Exception as error:
                    self._json({'error': str(error)}, 400)
                return
            actions = {'/api/checkpoint': 'checkpoint_current', '/api/mode-dial': 'mode_dial',
                       '/api/media': 'media_control', '/api/media/restart': 'restart_media'}
            if self.path not in actions:
                return super().do_POST()
            if not self._allowed():
                return
            action = getattr(session, actions[self.path], None)
            if not callable(action):
                self._json({'error': 'Not found'}, 404)
                return
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 4096:
                    raise ValueError('JSON body must be 1..4096 bytes')
                options = json.loads(self.rfile.read(length))
                if not isinstance(options, dict):
                    raise ValueError('Expected a JSON object')
                self._json(action(options))
            except Exception as error:
                self._json({'error': str(error)}, 400)

    server.RequestHandlerClass = MenuHandler
    return server
