# 领域 SDK

领域 SDK 是构建领域包所需的全部输入：包格式 Schema、处理器引导模块、引用索引实现和 `devdesk-domain` 命令行工具。构建一个领域包只需要 SDK 制品、领域包目录、Node 和 Python（带 jsonschema）——不需要内核源码、Web 源码或本仓库的脚本。

| 路径 | 内容 |
|---|---|
| `cli/devdesk-domain.mjs` | `init`、`check`、`build`、`verify` 四个命令 |
| `cli/usage-index.mjs` | 配置目录的引用索引；内核的检查脚本与领域包的构建共用这一份实现 |
| `lib/` | 包装配（读取 `domain.json` 与 `catalog/`）、引用索引、无依赖的 ZIP 读写 |
| `worker/bootstrap.mjs` | 处理器引导模块，与 Host 内置的是同一份 |
| `schemas/` | 包格式、贡献、对象类型与配置源 Schema |
| `kernel/` | 内核目录与工程扩展目录（构建工具从中读取处理器工具定义） |
| `catalog.json` | 发行时由 `devdesk-host capabilities --print` 生成的能力目录 |

```bash
./scripts/release/package.sh sdk          # 产出 devdesk-domain-sdk-<版本>.tar.gz
node sdk/domain/cli/devdesk-domain.mjs init <id>
node sdk/domain/cli/devdesk-domain.mjs check plugins/software-development --write
node sdk/domain/cli/devdesk-domain.mjs build plugins/software-development --out dist/
```

`check` 执行安装时校验的全部规则：格式与协议、Schema、扩展组协商、领域工具的命名空间与处理器、引用索引新鲜度（`--write` 刷新）。回归测试见 `node --test tests/entrypoints/domain_sdk.mjs`；发行与独立构建见[测试索引](../tests/README.md)。
