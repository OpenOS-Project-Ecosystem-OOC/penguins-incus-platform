"""Tests for EventBus fan-out."""

import asyncio

import pytest

from penguins_incus.events import EventBus


@pytest.mark.asyncio
async def test_publish_delivers_to_subscriber() -> None:
    bus = EventBus()
    q = bus.subscribe()
    event = {"type": "lifecycle", "project": "default", "metadata": {}}
    await bus.publish(event)
    received = q.get_nowait()
    assert received == event


@pytest.mark.asyncio
async def test_unsubscribe_stops_delivery() -> None:
    bus = EventBus()
    q = bus.subscribe()
    bus.unsubscribe(q)
    await bus.publish({"type": "test", "metadata": {}})
    assert q.empty()


@pytest.mark.asyncio
async def test_multiple_subscribers_all_receive() -> None:
    bus = EventBus()
    queues = [bus.subscribe() for _ in range(5)]
    event = {"type": "network", "metadata": {}}
    await bus.publish(event)
    for q in queues:
        assert q.get_nowait() == event


@pytest.mark.asyncio
async def test_full_queue_drops_event_without_raising() -> None:
    bus = EventBus()
    q = bus.subscribe()
    # Fill the queue to capacity
    for i in range(q.maxsize):
        await bus.publish({"type": "fill", "seq": i, "metadata": {}})
    # This should not raise even though queue is full
    await bus.publish({"type": "overflow", "metadata": {}})
    assert q.full()


@pytest.mark.asyncio
async def test_iter_events_type_filter() -> None:
    bus = EventBus()
    results: list = []

    async def _collect() -> None:
        async for event in bus.iter_events(type_filter="lifecycle"):
            results.append(event)
            if len(results) >= 2:
                break

    task = asyncio.create_task(_collect())
    await asyncio.sleep(0)  # yield to let task start

    await bus.publish({"type": "network",   "project": "", "metadata": {}})
    await bus.publish({"type": "lifecycle", "project": "", "metadata": {"action": "started"}})
    await bus.publish({"type": "logging",   "project": "", "metadata": {}})
    await bus.publish({"type": "lifecycle", "project": "", "metadata": {"action": "stopped"}})

    await asyncio.wait_for(task, timeout=2)
    assert len(results) == 2
    assert all(e["type"] == "lifecycle" for e in results)


@pytest.mark.asyncio
async def test_iter_events_project_filter() -> None:
    bus = EventBus()
    results: list = []

    async def _collect() -> None:
        async for event in bus.iter_events(project_filter="prod"):
            results.append(event)
            break

    task = asyncio.create_task(_collect())
    await asyncio.sleep(0)

    await bus.publish({"type": "lifecycle", "project": "dev",  "metadata": {}})
    await bus.publish({"type": "lifecycle", "project": "prod", "metadata": {}})

    await asyncio.wait_for(task, timeout=2)
    assert results[0]["project"] == "prod"
