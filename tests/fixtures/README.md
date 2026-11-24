# 测试夹具

本目录保存测试使用的固定数据，不作为产品运行时配置。

- `protocols/`：脱敏后的真实协议样本，由 Rust 协议回归测试读取；样本来源和对应测试见该目录的 README。
- [project-runtime/Dockerfile](project-runtime/Dockerfile) 构建隔离的 Node/Python 项目测试镜像。由 [prepare_project_test_runtime.py](../entrypoints/prepare_project_test_runtime.py) 调用。
- `configuration/retired-engineering.json`：退役详细版的不可变源输入，正文内联。仅由兼容性及旧平台工具合同测试显式装入独立测试库，用于保护历史恢复、出厂更新保留旧源及版本门禁；不属于 manifest，不嵌入生产程序，不作为可选出厂配置。

新增协议样本时记录供应商版本、脱敏方式和读取它的测试，并保持现有回放时序。


`configuration/engineering-catalog.json` 保存旧工程目录的冻结数据，供 `fixture-catalog` 的 Rust 和浏览器回归使用。正式 Host 只内嵌核心目录。 `domain-packages/software-development-0.3.0.zip` 是历史运行端版本，用来验证实际 ZIP 的回切与恢复。
