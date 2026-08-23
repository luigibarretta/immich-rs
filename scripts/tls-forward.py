#!/usr/bin/env python3
"""Terminate verified TLS on one private address and relay only to loopback."""

from __future__ import annotations

import argparse
import ipaddress
from pathlib import Path
import signal
import socket
import ssl
import threading

BUFFER_BYTES = 65_536
STOP = threading.Event()


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen-host", required=True)
    parser.add_argument("--listen-port", type=int, required=True)
    parser.add_argument("--target-port", type=int, required=True)
    parser.add_argument("--certificate", type=Path, required=True)
    parser.add_argument("--private-key", type=Path, required=True)
    values = parser.parse_args()
    try:
        listen_address = ipaddress.ip_address(values.listen_host)
    except ValueError as error:
        parser.error(f"listen host must be an IP address: {error}")
    if listen_address.is_loopback or not listen_address.is_private:
        parser.error("listen host must be a private non-loopback address")
    if not 1024 <= values.listen_port <= 65_535:
        parser.error("listen port must be unprivileged")
    if not 1024 <= values.target_port <= 65_535:
        parser.error("target port must be unprivileged")
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


def relay(client: socket.socket, target_port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", target_port), timeout=2) as target:
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


def serve(values: argparse.Namespace) -> None:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(values.certificate, values.private_key)
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((values.listen_host, values.listen_port))
        listener.listen(1)
        listener.settimeout(0.5)
        while not STOP.is_set():
            try:
                client, _address = listener.accept()
            except TimeoutError:
                continue
            try:
                secured = context.wrap_socket(client, server_side=True)
            except (OSError, ssl.SSLError):
                client.close()
                continue
            with secured:
                relay(secured, values.target_port)


def main() -> int:
    values = arguments()
    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    try:
        serve(values)
    except OSError:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
