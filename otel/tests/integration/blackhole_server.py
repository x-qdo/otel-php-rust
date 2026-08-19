import socketserver
import threading


class BlackholeHandler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        while self.request.recv(65536):
            pass


class BlackholeServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


servers = [BlackholeServer(("0.0.0.0", port), BlackholeHandler) for port in (4317, 4318)]
threads = [threading.Thread(target=server.serve_forever, daemon=True) for server in servers]

for thread in threads:
    thread.start()

for thread in threads:
    thread.join()
