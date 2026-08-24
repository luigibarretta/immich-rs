#!/usr/bin/env python3
"""Bound one disposable Immich target to a container driver's loopback."""

from __future__ import annotations

import argparse
import re
import signal
import socket
import sys
import threading

BUFFER_BYTES = 65_536
TARGET_PORT = 2283
TARGET_PATTERN = re.compile(
    r"^(?:immich-rs-[0-9TZ-]+-[0-9]+-[0-9a-f]{8}-server|"
    r"immichrs-(?:applephotos|picasa|mig-(?:src|dst))-"
    r"[0-9tz-]+-[0-9]+-[0-9a-f]{8}-server-1)$"
)
STOP = threading.Event()


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen-port", type=int, required=True)
    parser.add_argument("--target-container", required=True)
    values = parser.parse_args()
    if not 1024 <= values.listen_port <= 65_535:
        parser.error("listen port must be unprivileged")
    if not TARGET_PATTERN.fullmatch(values.target_container):
        parser.error("target must be a disposable Immich server container")
    return values


def request_stop(_signum: int, _frame: object) -> None:
    STOP.set()


def copy_stream(source: socket.socket, destination: socket.socket) -> None:
    try:
        while not STOP.is_set():
            try:
                chunk = source.recv(BUFFER_BYTES)
            except TimeoutError:
                continue
            if not chunk:
                break
            destination.sendall(chunk)
    except OSError:
        pass
    finally:
        try:
            destination.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def relay(client: socket.socket, target_container: str) -> None:
    try:
        with socket.create_connection((target_container, TARGET_PORT), timeout=2) as target:
            client.settimeout(0.5)
            target.settimeout(0.5)
            workers = (
                threading.Thread(target=copy_stream, args=(client, target)),
                threading.Thread(target=copy_stream, args=(target, client)),
            )
            for worker in workers:
                worker.start()
            for worker in workers:
                worker.join()
    except OSError:
        return


def serve(listen_port: int, target_container: str) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", listen_port))
        listener.listen(1)
        listener.settimeout(0.5)
        while not STOP.is_set():
            try:
                client, _address = listener.accept()
            except TimeoutError:
                continue
            with client:
                relay(client, target_container)


def main() -> int:
    values = arguments()
    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    try:
        serve(values.listen_port, values.target_container)
    except OSError as error:
        print(f"loopback forwarder failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
