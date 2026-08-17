import pytest
from fastapi.testclient import TestClient

from app.main import app


@pytest.fixture
def client():
    return TestClient(app)


def test_health_check(client):
    response = client.get("/health")
    assert response.status_code == 200
    data = response.json()
    assert data["status"] == "ok"
    assert "version" in data
    assert "uptime_seconds" in data


def test_classify_endpoint(client):
    response = client.post("/classify")
    assert response.status_code == 200


def test_index_build(client):
    response = client.post("/index/build")
    assert response.status_code == 200


def test_embedding_models(client):
    response = client.get("/embedding/models")
    assert response.status_code == 200
    assert "models" in response.json()
