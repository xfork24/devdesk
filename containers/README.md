# 容器资源

`tests/Dockerfile` 是不含模型凭据的 Node/Python 项目测试镜像，由
`tests/entrypoints/prepare_project_test_runtime.py` 构建。

Codex 和 DSH 的执行 Dockerfile 由
[`compose_execution_recipe`](../crates/domain/src/execution_recipe.rs)
根据选定的 Harness 安装输入与 Sandbox 构建输入生成，不再另存静态副本。
版本、基础镜像、系统包、环境变量及安装文件来自
[`plugins/core/manifest.json`](../plugins/core/manifest.json)
列出的源修订。实际准备、内容摘要和镜像身份核验见
[运行时准备](../docs/crates/devdesk-adapters/runtime-preparation.md)。
