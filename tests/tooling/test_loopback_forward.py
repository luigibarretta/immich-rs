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
        self.assertIsNotNone(
            pattern.fullmatch(
                "immichrs-applephotos-20260824t180932z-3094-460c937f-server-1"
            )
        )
        migration = "immichrs-mig-src-20260824t180932z-3094-460c937f-server-1"
        self.assertLessEqual(len(migration), 63)
        self.assertIsNotNone(pattern.fullmatch(migration))
        self.assertIsNotNone(
            pattern.fullmatch(
                "immichrs-mig-dst-20260824t180932z-3094-460c937f-server-1"
            )
        )
        self.assertIsNone(pattern.fullmatch("production-immich"))
        self.assertIsNone(
            pattern.fullmatch(
                "immichrs-applephotos-20260824T180932Z-3094-460c937f-server-1"
            )
        )
        self.assertIsNone(
            pattern.fullmatch(
                "immichrs-production-20260824t180932z-3094-460c937f-server-1"
            )
        )
        self.assertIsNone(
            pattern.fullmatch(
                "immichrs-migration-source-20260824t180932z-3094-460c937f-server-1"
            )
        )
        self.assertEqual(FORWARDER["TARGET_PORT"], 2283)
        self.assertEqual(FORWARDER["BUFFER_BYTES"], 65_536)

    def test_copy_stream_transfers_more_than_one_buffer(self) -> None:
        source_reader, source_writer = socket.socketpair()
        destination_reader, destination_writer = socket.socketpair()
        for endpoint in (source_reader, source_writer, destination_reader, destination_writer):
            self.addCleanup(endpoint.close)
        source_writer.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 8_192)
        destination_writer.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 8_192)
        destination_reader.settimeout(5)
        payload = bytes(range(251)) * 700
        send_errors: list[OSError] = []

        def send_payload() -> None:
            try:
                source_writer.sendall(payload)
                source_writer.shutdown(socket.SHUT_WR)
            except OSError as error:
                send_errors.append(error)

        worker = threading.Thread(
            target=FORWARDER["copy_stream"], args=(source_reader, destination_writer)
        )
        sender = threading.Thread(target=send_payload)
        worker.start()
        sender.start()
        received = bytearray()
        while chunk := destination_reader.recv(65_536):
            received.extend(chunk)
        sender.join(timeout=2)
        worker.join(timeout=2)
        self.assertFalse(sender.is_alive())
        self.assertFalse(worker.is_alive())
        self.assertEqual(send_errors, [])
        self.assertEqual(bytes(received), payload)


if __name__ == "__main__":
    unittest.main()
