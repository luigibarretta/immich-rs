"""Contract tests for the bounded disposable loopback forwarder."""

from __future__ import annotations

from pathlib import Path
import runpy
import socket
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]
FORWARDER = runpy.run_path(str(ROOT / "scripts" / "loopback-forward.py"))


class LoopbackForwardTests(unittest.TestCase):
    def test_target_pattern_accepts_only_disposable_server_names(self) -> None:
        pattern = FORWARDER["TARGET_PATTERN"]
        self.assertIsNotNone(pattern.fullmatch("immich-rs-20260815T010203Z-42-deadbeef-server"))
        self.assertIsNone(pattern.fullmatch("production-immich"))
        self.assertEqual(FORWARDER["TARGET_PORT"], 2283)
        self.assertEqual(FORWARDER["BUFFER_BYTES"], 65_536)

    def test_copy_stream_transfers_more_than_one_buffer(self) -> None:
        source_reader, source_writer = socket.socketpair()
        destination_reader, destination_writer = socket.socketpair()
        payload = bytes(range(251)) * 700
        worker = threading.Thread(
            target=FORWARDER["copy_stream"], args=(source_reader, destination_writer)
        )
        worker.start()
        source_writer.sendall(payload)
        source_writer.shutdown(socket.SHUT_WR)
        received = bytearray()
        while chunk := destination_reader.recv(65_536):
            received.extend(chunk)
        worker.join(timeout=2)
        for endpoint in (source_reader, source_writer, destination_reader, destination_writer):
            endpoint.close()
        self.assertFalse(worker.is_alive())
        self.assertEqual(bytes(received), payload)


if __name__ == "__main__":
    unittest.main()
