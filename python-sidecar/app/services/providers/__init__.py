"""T8.x — LLM Provider 实现包（Ollama / OpenAI / DeepSeek）。

每个 Provider 一个模块，实现 :class:`~app.services.cloud_provider.LLMProvider`
接口；T8.5 提供按推理模式选择 Provider 的工厂。
"""
