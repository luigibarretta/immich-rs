"""Contract tests for the bounded disposable TLS forwarder."""

from __future__ import annotations

from pathlib import Path
import runpy
import socket
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]
FORWARDER = runpy.run_path(str(ROOT / "scripts" / "tls-forward.py"))


class TlsForwardTests(unittest.TestCase):
    def test_copy_stream_transfers_more_than_one_buffer(self) -> None:
        source_reader, source_writer = socket.socketpair()
        destination_reader, destination_writer = socket.socketpair()
        for endpoint in (source_reader, source_writer, destination_reader, destination_writer):
            self.addCleanup(endpoint.close)
        source_writer.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 8_192)
        destination_writer.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 8_192)
        destination_reader.settimeout(5)
        payload = bytes(range(251)) * 700

        sender = threading.Thread(target=source_writer.sendall, args=(payload,))
        worker = threading.Thread(
            target=FORWARDER["copy_stream"], args=(source_reader, destination_writer)
        )
        worker.start()
        sender.start()
        received = bytearray()
        while len(received) < len(payload):
            received.extend(destination_reader.recv(65_536))
        source_writer.shutdown(socket.SHUT_WR)
        sender.join(timeout=2)
        worker.join(timeout=2)
        self.assertFalse(sender.is_alive())
        self.assertEqual(bytes(received), payload)

    def test_forwarder_uses_bounded_io_and_a_fixed_loopback_target(self) -> None:
        self.assertEqual(FORWARDER["BUFFER_BYTES"], 65_536)
        relay_names = set(FORWARDER["relay"].__code__.co_consts)
        self.assertIn("127.0.0.1", relay_names)


if __name__ == "__main__":
    unittest.main()
