import http.server
import json
import os
import socket
import socketserver
import sys
import threading
import time
import urllib.parse


def encode_varint(n):
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def encode_partial_success(rejected, message):
    inner = b"\x08" + encode_varint(rejected) + b"\x12" + encode_varint(len(message)) + message
    return b"\x0a" + encode_varint(len(inner)) + inner


_log_lock = threading.Lock()
_log_file = None


def init_log(path):
    global _log_file
    try:
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    except OSError:
        pass
    _log_file = open(path, "a", buffering=1)


def log_record(record):
    record = dict(record)
    record["ts"] = time.time()
    line = json.dumps(record)
    with _log_lock:
        _log_file.write(line + "\n")
        _log_file.flush()


def collect_headers(message):
    headers = {}
    for name, value in message.items():
        key = name.lower()
        if key in headers:
            headers[key] = headers[key] + ", " + value
        else:
            headers[key] = value
    return headers


class OTLPHTTPHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):
        pass

    def do_GET(self):
        self._handle("GET")

    def do_POST(self):
        self._handle("POST")

    def _handle(self, method):
        if method == "GET" and self.path == "/healthz":
            body = b"ok"
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        try:
            content_length = int(self.headers.get("Content-Length", 0) or 0)
        except ValueError:
            content_length = 0
        body = self.rfile.read(content_length) if content_length else b""

        path = self.path
        mode = "ok"
        if path.startswith("/mode/"):
            mode = path[len("/mode/"):].split("/", 1)[0] or "ok"

        status = self._respond(mode)

        log_record({
            "role": "http",
            "method": method,
            "path": path,
            "mode": mode,
            "headers": collect_headers(self.headers),
            "content_length": content_length,
            "body_gzip": body[:2] == b"\x1f\x8b",
            "body_len": len(body),
            "status": status,
        })

    def _respond(self, mode):
        if mode.startswith("delay-"):
            # Slow collector: hold the request for the given milliseconds, then accept.
            try:
                time.sleep(int(mode[len("delay-"):]) / 1000.0)
            except ValueError:
                pass
            self.send_response(200)
            self.send_header("Content-Type", "application/x-protobuf")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return 200

        if mode.startswith("partial-"):
            try:
                rejected = int(mode[len("partial-"):])
            except ValueError:
                rejected = 0
            # rejected == 0 with an empty message is the "nothing to report" form the
            # specification allows servers to send.
            message = f"fixture rejected {rejected} spans".encode() if rejected else b""
            payload = encode_partial_success(rejected, message)
            self.send_response(200)
            self.send_header("Content-Type", "application/x-protobuf")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return 200

        if mode.startswith("status-"):
            try:
                code = int(mode[len("status-"):])
            except ValueError:
                code = 200
            text = f"fixture status {code}".encode()
            self.send_response(code)
            self.send_header("Content-Type", "text/plain")
            if code in (429, 503):
                self.send_header("Retry-After", "1")
            self.send_header("Content-Length", str(len(text)))
            self.end_headers()
            self.wfile.write(text)
            return code

        self.send_response(200)
        self.send_header("Content-Type", "application/x-protobuf")
        self.send_header("Content-Length", "0")
        self.end_headers()
        return 200


class OTLPHTTPServer(http.server.ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True


class Http2Parser:
    def __init__(self):
        self.buf = b""
        self.preface_done = False
        self.seen_streams = set()
        self.frame_count = 0

    def feed(self, data):
        self.buf += data
        if not self.preface_done:
            if len(self.buf) < 24:
                return
            self.buf = self.buf[24:]
            self.preface_done = True
        self._parse_frames()

    def _parse_frames(self):
        while len(self.buf) >= 9:
            length = int.from_bytes(self.buf[0:3], "big")
            frame_type = self.buf[3]
            flags = self.buf[4]
            stream_id = int.from_bytes(self.buf[5:9], "big") & 0x7FFFFFFF
            if len(self.buf) < 9 + length:
                return
            payload = self.buf[9:9 + length]
            self.buf = self.buf[9 + length:]
            self.frame_count += 1
            self._handle_frame(frame_type, flags, stream_id, payload)

    def _handle_frame(self, frame_type, flags, stream_id, payload):
        if frame_type != 0x0 or stream_id in self.seen_streams:
            return
        self.seen_streams.add(stream_id)

        data = payload
        if flags & 0x8:
            if len(data) < 1:
                return
            pad_len = data[0]
            data = data[1:len(data) - pad_len] if pad_len else data[1:]

        if len(data) < 5:
            return
        compressed_flag = data[0]
        message_length = int.from_bytes(data[1:5], "big")
        gzip_magic = data[5:7] == b"\x1f\x8b" if len(data) >= 7 else None

        log_record({
            "role": "relay",
            "event": "grpc_message",
            "stream": stream_id,
            "compressed_flag": compressed_flag,
            "message_length": message_length,
            "gzip_magic": gzip_magic,
        })


def pump(src, dst, counters, key, parser):
    total = 0
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            total += len(data)
            if parser is not None:
                try:
                    parser.feed(data)
                except Exception as e:
                    log_record({"role": "relay", "event": "parse_error", "error": str(e)})
            try:
                dst.sendall(data)
            except OSError:
                break
    except OSError:
        pass
    finally:
        counters[key] = total
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle_relay(client_sock, upstream_host, upstream_port):
    try:
        upstream_sock = socket.create_connection((upstream_host, upstream_port), timeout=5)
    except OSError:
        log_record({"role": "relay", "event": "upstream_unreachable"})
        try:
            client_sock.close()
        except OSError:
            pass
        return

    parser = Http2Parser()
    counters = {}
    t1 = threading.Thread(target=pump, args=(client_sock, upstream_sock, counters, "c2u", parser), daemon=True)
    t2 = threading.Thread(target=pump, args=(upstream_sock, client_sock, counters, "u2c", None), daemon=True)
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    try:
        client_sock.close()
    except OSError:
        pass
    try:
        upstream_sock.close()
    except OSError:
        pass

    log_record({
        "role": "relay",
        "event": "connection_closed",
        "client_to_upstream_bytes": counters.get("c2u", 0),
        "upstream_to_client_bytes": counters.get("u2c", 0),
        "frames": parser.frame_count,
    })


class RelayHandler(socketserver.BaseRequestHandler):
    def handle(self):
        handle_relay(self.request, self.server.upstream_host, self.server.upstream_port)


class RelayServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, address, handler_cls, upstream_host, upstream_port):
        super().__init__(address, handler_cls)
        self.upstream_host = upstream_host
        self.upstream_port = upstream_port


def parse_headers(lines):
    headers = {}
    for line in lines:
        if b":" not in line:
            continue
        name, _, value = line.partition(b":")
        name = name.strip().lower().decode("latin-1")
        value = value.strip().decode("latin-1")
        if name in headers:
            headers[name] = headers[name] + ", " + value
        else:
            headers[name] = value
    return headers


_DROPPED_HEADERS = (b"proxy-connection", b"proxy-authorization", b"connection")


class ProxyHandler(socketserver.BaseRequestHandler):
    def handle(self):
        sock = self.request
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = sock.recv(65536)
            if not chunk:
                return
            buf += chunk
            if len(buf) > 262144:
                self._bad_request(sock)
                return

        head, _, rest = buf.partition(b"\r\n\r\n")
        lines = head.split(b"\r\n")
        request_line = lines[0].decode("latin-1", "replace")
        parts = request_line.split(" ")
        if len(parts) != 3:
            self._bad_request(sock)
            return
        method, target, version = parts
        header_lines = lines[1:]
        headers = parse_headers(header_lines)

        if method == "CONNECT":
            self._handle_connect(sock, target, rest)
            return

        if target.startswith("http://") or target.startswith("https://"):
            self._handle_absolute(sock, method, target, version, headers, header_lines, rest)
            return

        self._bad_request(sock)

    def _bad_request(self, sock):
        try:
            sock.sendall(b"HTTP/1.1 400 Bad Request\r\n\r\n")
        except OSError:
            pass
        log_record({"role": "proxy", "event": "bad_request"})

    def _handle_connect(self, sock, target, rest):
        try:
            host, port_str = target.rsplit(":", 1)
            port = int(port_str)
        except ValueError:
            self._bad_request(sock)
            return

        try:
            upstream = socket.create_connection((host, port), timeout=5)
        except OSError:
            try:
                sock.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            except OSError:
                pass
            return

        sock.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        log_record({"role": "proxy", "method": "CONNECT", "target": target})

        if rest:
            try:
                upstream.sendall(rest)
            except OSError:
                pass

        self._tunnel(sock, upstream)

    def _tunnel(self, a, b):
        counters = {}
        t1 = threading.Thread(target=pump, args=(a, b, counters, "1", None), daemon=True)
        t2 = threading.Thread(target=pump, args=(b, a, counters, "2", None), daemon=True)
        t1.start()
        t2.start()
        t1.join()
        t2.join()
        try:
            a.close()
        except OSError:
            pass
        try:
            b.close()
        except OSError:
            pass

    def _handle_absolute(self, sock, method, target, version, headers, header_lines, rest):
        parsed = urllib.parse.urlsplit(target)
        host = parsed.hostname
        port = parsed.port or 80
        path = parsed.path or "/"
        if parsed.query:
            path = path + "?" + parsed.query

        try:
            upstream = socket.create_connection((host, port), timeout=5)
        except OSError:
            try:
                sock.sendall(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            except OSError:
                pass
            return

        try:
            content_length = int(headers.get("content-length", 0) or 0)
        except ValueError:
            content_length = 0

        body = rest
        while len(body) < content_length:
            chunk = sock.recv(65536)
            if not chunk:
                break
            body += chunk

        out_lines = [f"{method} {path} {version}".encode("latin-1")]
        for line in header_lines:
            name = line.split(b":", 1)[0].strip().lower()
            if name in _DROPPED_HEADERS:
                continue
            out_lines.append(line)
        out_lines.append(b"Connection: close")
        out_head = b"\r\n".join(out_lines) + b"\r\n\r\n"

        try:
            upstream.sendall(out_head)
            if body:
                upstream.sendall(body)
        except OSError:
            try:
                upstream.close()
            except OSError:
                pass
            try:
                sock.close()
            except OSError:
                pass
            return

        log_record({
            "role": "proxy",
            "method": method,
            "target": target,
            "host": headers.get("host", ""),
        })

        try:
            while True:
                data = upstream.recv(65536)
                if not data:
                    break
                sock.sendall(data)
        except OSError:
            pass
        finally:
            try:
                upstream.close()
            except OSError:
                pass
            try:
                sock.close()
            except OSError:
                pass


class ProxyServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


log_path = os.environ.get("FIXTURE_LOG", "/var/lib/otel/fixture.jsonl")
init_log(log_path)

relay_upstream = os.environ.get("RELAY_UPSTREAM", "collector:4317")
upstream_host, upstream_port_str = relay_upstream.rsplit(":", 1)
upstream_port = int(upstream_port_str)

http_server = OTLPHTTPServer(("0.0.0.0", 4318), OTLPHTTPHandler)
relay_server = RelayServer(("0.0.0.0", 4317), RelayHandler, upstream_host, upstream_port)
proxy_server = ProxyServer(("0.0.0.0", 3128), ProxyHandler)

servers = [http_server, relay_server, proxy_server]
threads = [threading.Thread(target=server.serve_forever, daemon=True) for server in servers]

for thread in threads:
    thread.start()

print(
    f"otlp_transport_fixture: http=4318 relay=4317->{relay_upstream} proxy=3128 log={log_path}",
    file=sys.stderr,
    flush=True,
)

for thread in threads:
    thread.join()
