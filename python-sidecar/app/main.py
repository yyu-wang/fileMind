from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.api import routes_chat, routes_classify, routes_embedding, routes_health, routes_index

app = FastAPI(
    title="FileMind Sidecar",
    version="0.1.0",
    description="Python Sidecar for FileMind — classification + RAG + embedding",
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:1420"],
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(routes_health.router)
app.include_router(routes_classify.router)
app.include_router(routes_index.router)
app.include_router(routes_chat.router)
app.include_router(routes_embedding.router)
