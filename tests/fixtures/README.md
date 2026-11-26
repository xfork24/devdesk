# 测试夹具

本目录保存测试使用的固定数据，不作为产品运行时配置。

- `protocols/`：脱敏后的真实协议样本，由 Rust 协议回归测试读取；样本来源和对应测试见该目录的 README。
- [project-runtime/Dockerfile](project-runtime/Dockerfile) 构建隔离的 Node/Python 项目测试镜像。由 [prepare_project_test_runtime.py](../entrypoints/prepare_project_test_runtime.py) 调用。
- `reporters/`：内核测试执行器回归使用的冻结测试报告器（Node 与 Python），内容取自软件开发包 0.5.2 的同名文件，摘要写在 `crates/adapters/src/tests.rs`。领域包提供各自的报告器，这份副本不随领域包演进。
- `configuration/retired-engineering.json`：退役详细版的不可变源输入，正文内联。仅由兼容性及旧平台工具合同测试显式装入独立测试库，用于保护历史恢复、出厂更新保留旧源及版本门禁；不属于 manifest，不嵌入生产程序，不作为可选出厂配置。

新增协议样本时记录供应商版本、脱敏方式和读取它的测试，并保持现有回放时序。


`configuration/engineering-catalog.json` 保存旧工程目录的冻结数据，供 `fixture-catalog` 的 Rust 和浏览器回归使用。正式 Host 只内嵌核心目录。 `domain-packages/software-development-current.zip` 是当前格式的软件开发包（只含运行端与配置，不含 Web），供需要真实目录内容的 Rust 测试安装；清单格式变化后用 `python3 plugins/software-development/build.py --runtime-only tests/fixtures/domain-packages/software-development-current.zip` 重新生成。
