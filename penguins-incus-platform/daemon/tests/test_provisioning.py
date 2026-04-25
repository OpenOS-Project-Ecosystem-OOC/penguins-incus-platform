"""Tests for Compose provisioning logic."""

from penguins_incus.provisioning.compose import convert_compose


SIMPLE_COMPOSE = """
services:
  web:
    image: nginx:latest
    ports:
      - "8080:80"
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: secret
volumes:
  pgdata: {}
"""

MULTI_PORT_COMPOSE = """
services:
  app:
    image: myapp:latest
    ports:
      - "3000:3000"
      - "3001:3001"
"""

VOLUME_COMPOSE = """
services:
  app:
    image: myapp:latest
    volumes:
      - appdata:/data
volumes:
  appdata: {}
"""


def test_convert_simple_compose() -> None:
    result = convert_compose(SIMPLE_COMPOSE)
    assert "services" in result
    assert "web" in result["services"]
    assert "db" in result["services"]


def test_convert_maps_ports_to_proxies() -> None:
    result = convert_compose(SIMPLE_COMPOSE)
    web = result["services"]["web"]
    proxies = web["devices"]["proxies"]
    assert len(proxies) == 1
    assert proxies[0]["listen"]  == "tcp:127.0.0.1:8080"
    assert proxies[0]["connect"] == "tcp:0.0.0.0:80"


def test_convert_multiple_ports() -> None:
    result = convert_compose(MULTI_PORT_COMPOSE)
    proxies = result["services"]["app"]["devices"]["proxies"]
    assert len(proxies) == 2
    ports = {p["listen"].split(":")[-1] for p in proxies}
    assert ports == {"3000", "3001"}


def test_convert_maps_volumes() -> None:
    result = convert_compose(VOLUME_COMPOSE)
    vols = result["services"]["app"]["volumes"]
    assert len(vols) == 1
    assert vols[0]["source"] == "appdata"
    assert vols[0]["target"] == "/data"


def test_convert_preserves_top_level_volumes() -> None:
    result = convert_compose(SIMPLE_COMPOSE)
    assert "pgdata" in result["volumes"]


def test_convert_maps_image() -> None:
    result = convert_compose(SIMPLE_COMPOSE)
    assert result["services"]["web"]["image"] == "docker:nginx:latest"


def test_convert_invalid_yaml_returns_error() -> None:
    result = convert_compose("{ invalid: yaml: [")
    assert "error" in result


def test_convert_empty_services() -> None:
    result = convert_compose("services: {}")
    assert result["services"] == {}


def test_convert_environment_preserved() -> None:
    result = convert_compose(SIMPLE_COMPOSE)
    db = result["services"]["db"]
    assert "environment" in db
    assert db["environment"]["POSTGRES_PASSWORD"] == "secret"
