"""HTTP client wrapper used by CLI commands."""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import signal
import sys
import termios
import tty
from typing import Any
from urllib.parse import urlencode

import httpx
import websockets
import websockets.exceptions
from rich.console import Console

console = Console()


class DaemonClient:
    def __init__(self, base_url: str) -> None:
        self._base = base_url.rstrip("/")
        self._http = httpx.Client(base_url=self._base, timeout=30)

    def _handle(self, resp: httpx.Response) -> Any:
        try:
            resp.raise_for_status()
        except httpx.HTTPStatusError as exc:
            console.print(f"[red]Error {exc.response.status_code}:[/] {exc.response.text}")
            sys.exit(1)
        data = resp.json()
        console.print_json(json.dumps(data))
        return data

    def get(self, path: str, **kwargs: Any) -> Any:
        return self._handle(self._http.get(path, **kwargs))

    def post(self, path: str, **kwargs: Any) -> Any:
        return self._handle(self._http.post(path, **kwargs))

    def put(self, path: str, **kwargs: Any) -> Any:
        return self._handle(self._http.put(path, **kwargs))

    def delete(self, path: str, **kwargs: Any) -> Any:
        return self._handle(self._http.delete(path, **kwargs))

    def get_text(self, path: str, **kwargs: Any) -> None:
        """Fetch a plain-text response (e.g. logs) and print to stdout."""
        resp = self._http.get(path, **kwargs)
        try:
            resp.raise_for_status()
        except httpx.HTTPStatusError as exc:
            console.print(f"[red]Error {exc.response.status_code}:[/] {exc.response.text}")
            sys.exit(1)
        console.print(resp.text)

    def download_file(self, path: str, params: dict[str, str], dest: str) -> None:
        """Download binary content from *path* and write to *dest*."""
        resp = self._http.get(path, params=params)
        try:
            resp.raise_for_status()
        except httpx.HTTPStatusError as exc:
            console.print(f"[red]Error {exc.response.status_code}:[/] {exc.response.text}")
            sys.exit(1)
        with open(dest, "wb") as fh:
            fh.write(resp.content)
        console.print(f"[green]Saved[/] {dest} ({len(resp.content)} bytes)")

    def upload_file(self, path: str, params: dict[str, str], src: str) -> None:
        """Read *src* and POST its bytes to *path*."""
        with open(src, "rb") as fh:
            data = fh.read()
        resp = self._http.post(
            path, params=params, content=data,
            headers={"Content-Type": "application/octet-stream"},
        )
        try:
            resp.raise_for_status()
        except httpx.HTTPStatusError as exc:
            console.print(f"[red]Error {exc.response.status_code}:[/] {exc.response.text}")
            sys.exit(1)
        console.print(f"[green]Uploaded[/] {src} ({len(data)} bytes)")

    def exec_session(
        self,
        name: str,
        command: str = "/bin/bash",
        project: str = "",
    ) -> None:
        """Open an interactive PTY session inside *name* via the daemon WebSocket.

        Puts the local terminal into raw mode for the duration of the session,
        forwards SIGWINCH (terminal resize) to the daemon, and restores the
        terminal on exit regardless of how the session ends.
        """
        if not sys.stdin.isatty():
            console.print("[red]exec requires an interactive terminal[/]")
            sys.exit(1)

        cols, rows = shutil.get_terminal_size((80, 24))
        base_ws = self._base.replace("http://", "ws://").replace("https://", "wss://")
        params: dict[str, str] = {"command": command, "width": str(cols), "height": str(rows)}
        if project:
            params["project"] = project
        url = f"{base_ws}/api/v1/instances/{name}/exec/ws?{urlencode(params)}"

        asyncio.run(self._exec_async(url, name))

    async def _exec_async(self, url: str, name: str) -> None:
        """Async core of exec_session: raw-mode I/O loop with resize support."""
        fd = sys.stdin.fileno()
        old_settings = termios.tcgetattr(fd)

        loop = asyncio.get_running_loop()

        try:
            tty.setraw(fd)

            async with websockets.connect(  # type: ignore[attr-defined]
                url,
                ping_interval=20,
                ping_timeout=10,
            ) as ws:

                # ── SIGWINCH: send resize escape sequence to daemon ────────
                def _on_resize() -> None:
                    c, r = shutil.get_terminal_size((80, 24))
                    # VT100 resize sequence — the daemon's PTY honours it
                    seq = f"\x1b[8;{r};{c}t"
                    asyncio.ensure_future(ws.send(seq.encode()))

                loop.add_signal_handler(signal.SIGWINCH, _on_resize)

                # ── stdin → WebSocket ─────────────────────────────────────
                async def _stdin_to_ws() -> None:
                    try:
                        while True:
                            chunk = await loop.run_in_executor(
                                None, lambda: os.read(fd, 256)
                            )
                            if not chunk:
                                break
                            await ws.send(chunk)
                    except (OSError, websockets.exceptions.ConnectionClosed):
                        pass

                # ── WebSocket → stdout ────────────────────────────────────
                async def _ws_to_stdout() -> None:
                    try:
                        async for msg in ws:
                            data = msg if isinstance(msg, bytes) else msg.encode()
                            os.write(sys.stdout.fileno(), data)
                    except websockets.exceptions.ConnectionClosed:
                        pass

                done, pending = await asyncio.wait(
                    [
                        asyncio.ensure_future(_stdin_to_ws()),
                        asyncio.ensure_future(_ws_to_stdout()),
                    ],
                    return_when=asyncio.FIRST_COMPLETED,
                )
                for task in pending:
                    task.cancel()

        except websockets.exceptions.WebSocketException as exc:
            # Restore terminal before printing so the error is readable
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
            console.print(f"\n[red]WebSocket error:[/] {exc}")
            sys.exit(1)
        finally:
            try:
                loop.remove_signal_handler(signal.SIGWINCH)
            except Exception:
                pass
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
            # Ensure the cursor is on a fresh line after the session ends
            sys.stdout.write("\r\n")
            sys.stdout.flush()

    def stream_events(self, event_type: str = "") -> None:
        """Stream SSE events from the daemon, printing each to stdout."""
        params = {"type": event_type} if event_type else {}
        with self._http.stream("GET", "/api/v1/events", params=params) as resp:
            for line in resp.iter_lines():
                if line.startswith("data:"):
                    payload = line[5:].strip()
                    try:
                        console.print_json(payload)
                    except Exception:
                        console.print(payload)
