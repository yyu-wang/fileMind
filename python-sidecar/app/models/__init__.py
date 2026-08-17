from pydantic import BaseModel


class HealthResponse(BaseModel):
    status: str
    version: str
    uptime_seconds: float
