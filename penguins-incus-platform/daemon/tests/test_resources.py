"""Tests for resource usage parsing and CPU diff logic."""

from penguins_incus.resources import (
    _CpuSample,
    _parse_disk,
    _parse_memory,
    _read_cpu_ns,
    calc_cpu_fraction,
)


# ── calc_cpu_fraction ─────────────────────────────────────────────────────────

def test_cpu_fraction_zero_on_first_sample() -> None:
    # Simulate first-sample behaviour: caller passes prev == curr, expects 0.
    prev = _CpuSample(cpu_ns=0, wall_ns=0.0)
    assert calc_cpu_fraction(prev, curr_cpu_ns=0, curr_wall_ns=0.0) == 0.0


def test_cpu_fraction_zero_when_wall_delta_is_zero() -> None:
    prev = _CpuSample(cpu_ns=1_000_000, wall_ns=1_000_000_000.0)
    # Same wall time -> division by zero guard
    assert calc_cpu_fraction(prev, curr_cpu_ns=2_000_000, curr_wall_ns=1_000_000_000.0) == 0.0


def test_cpu_fraction_50_percent() -> None:
    # 1 CPU, 5 s elapsed (5e9 ns wall), 2.5 s CPU (2.5e9 ns) -> 50 %
    wall_delta = 5_000_000_000.0
    cpu_delta  = 2_500_000_000
    prev = _CpuSample(cpu_ns=0, wall_ns=0.0)
    result = calc_cpu_fraction(prev, curr_cpu_ns=cpu_delta, curr_wall_ns=wall_delta, num_cpus=1)
    assert abs(result - 0.5) < 1e-9


def test_cpu_fraction_scales_by_num_cpus() -> None:
    # 4 CPUs, 5 s elapsed, 5 s CPU -> 25 % per CPU
    wall_delta = 5_000_000_000.0
    cpu_delta  = 5_000_000_000
    prev = _CpuSample(cpu_ns=0, wall_ns=0.0)
    result = calc_cpu_fraction(prev, curr_cpu_ns=cpu_delta, curr_wall_ns=wall_delta, num_cpus=4)
    assert abs(result - 0.25) < 1e-9


def test_cpu_fraction_clamped_to_one() -> None:
    # CPU delta > wall delta (e.g. multi-core burst) -> clamp to 1.0
    prev = _CpuSample(cpu_ns=0, wall_ns=0.0)
    result = calc_cpu_fraction(
        prev, curr_cpu_ns=10_000_000_000, curr_wall_ns=1_000_000_000.0, num_cpus=1
    )
    assert result == 1.0


def test_cpu_fraction_non_zero_baseline() -> None:
    # Baseline is not zero -- only the delta matters
    prev = _CpuSample(cpu_ns=1_000_000_000, wall_ns=10_000_000_000.0)
    curr_cpu_ns  = 1_000_000_000 + 2_500_000_000   # +2.5 s CPU
    curr_wall_ns = 10_000_000_000.0 + 5_000_000_000.0  # +5 s wall
    result = calc_cpu_fraction(
        prev, curr_cpu_ns=curr_cpu_ns, curr_wall_ns=curr_wall_ns, num_cpus=1
    )
    assert abs(result - 0.5) < 1e-9


def test_cpu_fraction_zero_num_cpus_guard() -> None:
    prev = _CpuSample(cpu_ns=0, wall_ns=0.0)
    assert calc_cpu_fraction(prev, curr_cpu_ns=1_000_000, curr_wall_ns=1_000_000_000.0, num_cpus=0) == 0.0


# ── _read_cpu_ns ──────────────────────────────────────────────────────────────

def test_read_cpu_ns_zero_when_empty() -> None:
    assert _read_cpu_ns({}) == 0


def test_read_cpu_ns_returns_value() -> None:
    assert _read_cpu_ns({"cpu": {"usage": 123_456_789}}) == 123_456_789


# ── _parse_memory ─────────────────────────────────────────────────────────────

def test_parse_memory_zero_when_empty() -> None:
    assert _parse_memory({}) == 0


def test_parse_memory_returns_bytes() -> None:
    assert _parse_memory({"memory": {"usage": 536_870_912}}) == 536_870_912


# ── _parse_disk ───────────────────────────────────────────────────────────────

def test_parse_disk_zero_when_empty() -> None:
    assert _parse_disk({}) == 0


def test_parse_disk_sums_all_devices() -> None:
    state = {"disk": {"root": {"usage": 1000}, "data": {"usage": 2000}}}
    assert _parse_disk(state) == 3000


def test_parse_disk_single_device() -> None:
    assert _parse_disk({"disk": {"root": {"usage": 5000}}}) == 5000
