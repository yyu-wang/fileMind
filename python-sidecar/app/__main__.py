"""允许 ``python -m app`` 直接启动 Sidecar（CI 单测与一次性本地调试兜底，**不**用于 PyInstaller 打包）。

PyInstaller 入口是同目录下的 ``sidecar_entry.py``。``__main__.py`` 仅在
开发者未记入口名时提供备用路径，实现等价于 ``uvicorn app.main:app``，不做
PSK 环境变量注入（交给 app/main.py 的 lifespan 钩子按原逻辑处理）。
"""

from __future__ import annotations

from uvicorn import run

if __name__ == "__main__":
    run("app.main:app", host="127.0.0.1", port=8765, log_level="info")
