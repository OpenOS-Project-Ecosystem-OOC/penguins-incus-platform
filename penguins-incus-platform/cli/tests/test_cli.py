"""CLI command tests using Click's test runner and httpx mock transport."""

from __future__ import annotations

import json
from contextlib import contextmanager
from typing import Any, Generator
from unittest.mock import MagicMock, patch

import httpx
import pytest
from click.testing import CliRunner

from penguins_incus.cli.main import cli


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_DUMMY_REQUEST = httpx.Request("GET", "http://127.0.0.1:8765/")


def _mock_response(data: Any, status_code: int = 200) -> httpx.Response:
    # httpx.Response.raise_for_status() requires a request to be attached.
    return httpx.Response(
        status_code=status_code,
        content=json.dumps(data).encode(),
        headers={"content-type": "application/json"},
        request=_DUMMY_REQUEST,
    )


def _runner() -> CliRunner:
    # mix_stderr was removed in Click 8.2; stderr is separate by default
    return CliRunner()


def _invoke(args: list[str], mock_data: Any = None, status_code: int = 200) -> Any:
    """Invoke CLI with a mocked DaemonClient that returns *mock_data*."""
    runner = _runner()
    mock_resp = _mock_response(mock_data or [], status_code)

    with patch("penguins_incus.cli.client.httpx.Client") as MockClient:
        mock_http = MagicMock()
        mock_http.get.return_value = mock_resp
        mock_http.post.return_value = mock_resp
        mock_http.put.return_value = mock_resp
        mock_http.delete.return_value = mock_resp
        MockClient.return_value = mock_http
        result = runner.invoke(cli, args, catch_exceptions=False)

    return result, mock_http


# ---------------------------------------------------------------------------
# container list
# ---------------------------------------------------------------------------

def test_container_list_calls_instances_endpoint() -> None:
    result, mock_http = _invoke(["container", "list"], mock_data=[])
    assert result.exit_code == 0
    mock_http.get.assert_called_once()
    call_args = mock_http.get.call_args
    assert "/api/v1/instances" in call_args[0][0]


def test_container_list_passes_type_container() -> None:
    _, mock_http = _invoke(["container", "list"])
    params = mock_http.get.call_args[1].get("params", {})
    assert params.get("type") == "container"


def test_container_list_passes_project_option() -> None:
    _, mock_http = _invoke(["container", "list", "--project", "myproject"])
    params = mock_http.get.call_args[1].get("params", {})
    assert params.get("project") == "myproject"


# ---------------------------------------------------------------------------
# container create
# ---------------------------------------------------------------------------

def test_container_create_posts_to_instances() -> None:
    result, mock_http = _invoke(
        ["container", "create", "mybox", "--image", "images:ubuntu/24.04"],
        mock_data={"id": "op-123"},
    )
    assert result.exit_code == 0
    mock_http.post.assert_called_once()
    path = mock_http.post.call_args[0][0]
    assert "/api/v1/instances" in path


def test_container_create_sends_correct_body() -> None:
    _, mock_http = _invoke(
        ["container", "create", "mybox", "--image", "images:ubuntu/24.04",
         "--profile", "default", "--profile", "gpu"],
        mock_data={"id": "op-123"},
    )
    body = mock_http.post.call_args[1]["json"]
    assert body["name"] == "mybox"
    assert body["image"] == "images:ubuntu/24.04"
    assert body["type"] == "container"
    assert "default" in body["profiles"]
    assert "gpu" in body["profiles"]


# ---------------------------------------------------------------------------
# container start / stop / restart
# ---------------------------------------------------------------------------

def test_container_start_puts_state() -> None:
    _, mock_http = _invoke(["container", "start", "mybox"])
    mock_http.put.assert_called_once()
    path = mock_http.put.call_args[0][0]
    assert "mybox/state" in path
    body = mock_http.put.call_args[1]["json"]
    assert body["action"] == "start"


def test_container_stop_puts_state() -> None:
    _, mock_http = _invoke(["container", "stop", "mybox"])
    body = mock_http.put.call_args[1]["json"]
    assert body["action"] == "stop"


def test_container_stop_force_flag() -> None:
    _, mock_http = _invoke(["container", "stop", "--force", "mybox"])
    body = mock_http.put.call_args[1]["json"]
    assert body["force"] is True


def test_container_restart_puts_state() -> None:
    _, mock_http = _invoke(["container", "restart", "mybox"])
    body = mock_http.put.call_args[1]["json"]
    assert body["action"] == "restart"


# ---------------------------------------------------------------------------
# container delete
# ---------------------------------------------------------------------------

def test_container_delete_calls_delete() -> None:
    _, mock_http = _invoke(["container", "delete", "mybox"])
    mock_http.delete.assert_called_once()
    path = mock_http.delete.call_args[0][0]
    assert "mybox" in path


# ---------------------------------------------------------------------------
# vm list
# ---------------------------------------------------------------------------

def test_vm_list_passes_type_virtual_machine() -> None:
    _, mock_http = _invoke(["vm", "list"])
    params = mock_http.get.call_args[1].get("params", {})
    assert params.get("type") == "virtual-machine"


# ---------------------------------------------------------------------------
# network / storage / image / profile / project / cluster / remote / operation
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("args,expected_path", [
    (["network", "list"],   "/api/v1/networks"),
    (["storage", "list"],   "/api/v1/storage-pools"),
    (["image", "list"],     "/api/v1/images"),
    (["profile", "list"],   "/api/v1/profiles"),
    (["project", "list"],   "/api/v1/projects"),
    (["cluster", "list"],   "/api/v1/cluster/members"),
    (["remote", "list"],    "/api/v1/remotes"),
    (["operation", "list"], "/api/v1/operations"),
])
def test_list_commands_call_correct_endpoint(args: list[str], expected_path: str) -> None:
    result, mock_http = _invoke(args, mock_data=[])
    assert result.exit_code == 0
    path = mock_http.get.call_args[0][0]
    assert expected_path in path


# ---------------------------------------------------------------------------
# operation cancel
# ---------------------------------------------------------------------------

def test_operation_cancel_calls_delete() -> None:
    _, mock_http = _invoke(["operation", "cancel", "op-abc123"])
    mock_http.delete.assert_called_once()
    path = mock_http.delete.call_args[0][0]
    assert "op-abc123" in path


# ---------------------------------------------------------------------------
# error handling
# ---------------------------------------------------------------------------

def test_http_error_exits_nonzero() -> None:
    runner = _runner()
    error_resp = httpx.Response(
        status_code=500,
        content=b'{"error": "internal server error"}',
        headers={"content-type": "application/json"},
        request=httpx.Request("GET", "http://127.0.0.1:8765/api/v1/instances"),
    )

    with patch("penguins_incus.cli.client.httpx.Client") as MockClient:
        mock_http = MagicMock()
        mock_http.get.return_value = error_resp
        MockClient.return_value = mock_http
        result = runner.invoke(cli, ["container", "list"])

    assert result.exit_code != 0


# ---------------------------------------------------------------------------
# events streaming
# ---------------------------------------------------------------------------

def _make_sse_lines(payloads: list[dict[str, Any]]) -> list[str]:
    """Build SSE-formatted lines from a list of JSON payloads."""
    lines = []
    for p in payloads:
        lines.append(f"data: {json.dumps(p)}")
        lines.append("")  # blank line between events
    return lines


@contextmanager
def _mock_stream(lines: list[str]) -> Generator[MagicMock, None, None]:
    """Context manager that patches httpx.Client so .stream() yields *lines*."""
    mock_resp = MagicMock()
    mock_resp.iter_lines.return_value = iter(lines)
    mock_resp.__enter__ = lambda s: mock_resp
    mock_resp.__exit__ = MagicMock(return_value=False)

    with patch("penguins_incus.cli.client.httpx.Client") as MockClient:
        mock_http = MagicMock()
        mock_http.stream.return_value = mock_resp
        MockClient.return_value = mock_http
        yield mock_http


def test_events_calls_sse_endpoint() -> None:
    payloads = [{"type": "lifecycle", "metadata": {"action": "instance-started"}}]
    with _mock_stream(_make_sse_lines(payloads)) as mock_http:
        result = _runner().invoke(cli, ["events"], catch_exceptions=False)

    assert result.exit_code == 0
    mock_http.stream.assert_called_once()
    path = mock_http.stream.call_args[0][1]
    assert "/api/v1/events" in path


def test_events_prints_each_payload() -> None:
    payloads = [
        {"type": "lifecycle", "metadata": {"action": "instance-started"}},
        {"type": "lifecycle", "metadata": {"action": "instance-stopped"}},
    ]
    with _mock_stream(_make_sse_lines(payloads)):
        result = _runner().invoke(cli, ["events"], catch_exceptions=False)

    assert "instance-started" in result.output
    assert "instance-stopped" in result.output


def test_events_type_filter_passed_as_param() -> None:
    with _mock_stream([]) as mock_http:
        _runner().invoke(cli, ["events", "--type", "lifecycle"], catch_exceptions=False)

    params = mock_http.stream.call_args[1].get("params", {})
    assert params.get("type") == "lifecycle"


def test_events_no_type_filter_sends_no_param() -> None:
    with _mock_stream([]) as mock_http:
        _runner().invoke(cli, ["events"], catch_exceptions=False)

    params = mock_http.stream.call_args[1].get("params", {})
    assert not params  # empty dict — no type filter


def test_events_ignores_non_data_lines() -> None:
    """Lines that don't start with 'data:' must be silently skipped."""
    lines = [
        ": keep-alive",
        "event: lifecycle",
        f"data: {json.dumps({'type': 'lifecycle'})}",
        "",
    ]
    with _mock_stream(lines):
        result = _runner().invoke(cli, ["events"], catch_exceptions=False)

    assert result.exit_code == 0
    # Only the data line should produce output
    assert result.output.count("lifecycle") == 1


def test_events_handles_invalid_json_gracefully() -> None:
    """Malformed JSON in a data line must not crash the CLI."""
    lines = ["data: {not valid json}", ""]
    with _mock_stream(lines):
        result = _runner().invoke(cli, ["events"], catch_exceptions=False)

    assert result.exit_code == 0
    assert "{not valid json}" in result.output
