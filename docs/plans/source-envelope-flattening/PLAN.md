# 源配置信封扁平化与内部域模型对齐

日期：2026-11-13。状态：已完成（实现、离线验收、Codex 原生 Feature 业务交付与封存证据复核）。基准：main `58bb4d2`。

## 背景与目标

`resources/configuration` 的 151 个出厂源文件与整个运行时共用信封 `{kind, identity, revision, name, scope:{kind}, data}`，引用字段埋在 `data` 内。目标是一次性把 wire 格式与 Rust 内部表示统一对齐为新形状，不做边界翻译：

```json
{
  "type": "capability", "identity": "...", "revision": 1, "name": "...", "scope": "global",
  "harness_ref": "...:1", "model_ref": "...:1", "skills": ["...:2"], "tools": ["...:1"],
  "data": {}
}
```

三项字段级变更：`kind` 改名 `type`；`scope:{kind:"global"}` 压平为字符串 `"global"` / `"project:<id>"` / `"feature:<pid>:<fid>"`；`data` 顶层引用字段平铺到信封根级，其余载荷留在 `data`。

**引用白名单**（`domain/src/governance.rs: reference_fields`，serde 边界 / 存储 / 校验 / 源文件转换脚本四方保持一致）：

| type | 根级引用字段 |
| --- | --- |
| capability | harness_ref, model_ref, skills, tools |
| agent | role_ref, capability_ref |
| delivery_type | spec_refs |
| harness | model_providers, model_catalog_ref |
| permission_context | sandbox_ref |
| standard | root_workflow_refs |

不动的部分：model/harness/tool 的 `capabilities` 是能力标签（"tool-calling" 等）非引用，留在 data；嵌套引用（tool 的 `runtime.*`、delivery 的 `producer.tools`、`step_grants[]`、workflow step 内部）保持原位。

## 内部域模型变更（domain）

- **`Scope`** 从 internally-tagged 枚举改为 `#[serde(transparent)]` 字符串 newtype（`domain/src/governance.rs`）。构造器 `Scope::global()` / `Scope::project(id)` / `Scope::feature(pid, fid)`（泛型 `impl Into<String>`）；访问器 `is_global()` / `is_feature()` / `project_id()`；`visible_from` 按前缀语义等价。原 `project()` 访问器更名 `project_id()`，与构造器解除命名冲突。
- **`ConfigurationRecord`**：`kind` 字段改名 `r#type`；新增 `#[serde(flatten)] refs: References`（`BTreeMap<String, Value>`），derive serde 天然根级平铺。deny_unknown_fields 与 flatten 不兼容，严格性移到 `validate_references`（refs 键 ⊆ 白名单 + `identity:revision` 格式 + 列表去重）。
- **`content_hash` 重定义**：`content_digest(&refs, &data) = SHA-256((refs, data))`。capability/agent 的 data 变空后摘要仍覆盖能力组合，`composition_hash`、`model_profile_digest`、`template_source_hash`（default/modified 判定）语义保持。所有重算点（`update_record`、`pack_record`、`restore_records`、权限 profile 校验、测试 fixture）统一走 `content_digest`。
- **`SourceDefinition`** 同样改为 `r#type + scope: Scope + flatten refs + data`；`into_record()` 直接搬运。`validate_source_data(kind, refs, data)` 增加白名单与引用格式校验；`reference_id` / `reference_list` / `reference_value` 用 `refs.get()`（缺失当 JSON null），杜绝 BTreeMap 索引 panic。
- **类型化载荷瘦身**：`CapabilityDefinition` 与 `AgentDefinition` 删除（全字段已平铺）；`HarnessDefinition` 去掉 `model_providers`/`model_catalog_ref`；`DeliveryTypeDefinition` 去掉 `spec_refs`（`validate_delivery_specs` 改收 refs；body 改为只限长度，内容要求由根级 spec_refs 承担）；`PermissionContextDefinition` 去掉 `sandbox_ref`；`ConfiguredStandard` 只剩 `schema_version`，入口经 `resolve_standard_sources(state, roots, scope)` 传入。
- **`resolve_agent`**、`resolve_model_connection`/`provider_facts`（harness 参数改收 `&ConfigurationRecord`）均从 `record.refs` 读取。

## 命令与校验（core）

- `record_with_refs(...)` 新构造器（`record()` 空 refs 转发）；`*.create`/`*.revise` 从 params 根级按白名单提取引用键进 refs，`params["data"]` 只装剩余载荷；新增未知根级字段拒绝（伪造 `model_ref` 等被拒，保留原 deny_unknown_fields 语义）。scope 默认值从 `{"kind":"global"}` 对象改为 `"global"` 字符串。
- `check_scope`、`check_configured_workflow_scope` 从枚举匹配改前缀判断。`reference()`/`validate_composition`/`validate_capability`/`validate_harness`/`normalize_agent` 全部基于 refs。
- `templates.rs`：`load_bundle` 的 `_file` 内联照旧（作用于 data）；`is_source` 判定改为整条 `from_value::<ConfigurationRecord>(source.record) == r`；`pinned_template_records` 的引用闭包同时遍历 `refs` 与 `data`。`build.rs` 校验 `source["type"]` 与 manifest types 键一致。

## 存储（adapters + sql）

- `sql/project/schema.sql` 与 `sql/global/schema.sql`：`source_definitions` 列 `kind`→`type`、新增 `refs`（JSON）；`governance_records` 同样 `type` 列 + `refs` 列；全部 kind 索引/触发器改 type；`idx_governance_capability_reference` 改走 `json_extract(refs,'$.capability_ref')`。索引名 `idx_governance_kind` → `idx_governance_type`、`idx_source_kind` → `idx_source_type`。
- `source_store.rs`：手写 SQL 同步新列；runtime 记录在 `save_records`/`load_records` 边界做「存储形状」转换（`ConfigurationRecord::to_storage()` 把白名单根键收进 `refs` 键、`from_storage()` 反向展开），`governance_records` 的列式 `load_table` 从 `Value` 中转。`experience.rs`、`integrations.rs` 的旁路加载点同步。
- `lib.rs` raw SQL（`has_pending_work`）kind 列改 type。
- 验证只使用全新空数据目录；不添加旧数据迁移或兼容执行路径。数据库基线为 global 17 / project 48。

## 源文件与校验脚本

- `scripts/py/migrate-source-envelope.py`（一次性脚本，已执行）：151 个 JSON 完成 `kind`→`type`、scope 压平、白名单引用提升。
- `schemas/source-definition.schema.json`：required `[type,...]`，scope 为字符串 enum `"global"`，根级白名单引用字段（`reference`/`referenceList` $defs），additionalProperties false。
- `scripts/py/check-defaults.py`：信封字段表、`source["type"]` 匹配、scope 必须 global、根级引用解析到已发布修订。

## 收尾验收（已完成）

1. **Rust**：全工作区 236 项通过，15 项按既有环境要求忽略。`RUST_TEST_THREADS=2 cargo test --workspace --locked`；工作流套件另以默认并发复跑 37/37。
2. **发布合同**：源 / RPC / 权限 / 模板清单 Schema 与当前 serde、根级引用形状及普通整数元数据一致；结构接受不依赖版本号全等。
3. **Web**：全部记录读取、Scope 读写、根级引用编辑保存完成；TypeScript + Vite 通过；debug/release Playwright 各 51/51，含新增延迟读取竞态回归。
4. **文档与出厂资源**：协议说明、当前存储基线及计划索引同步；bundle `2026.11.13-source-flat-v1`，151 个源定义自洽。
5. **Host**：独立空目录初始化、重启、HTTP/连接安全、模板重置、运行准备和 standalone release 检查通过。没有操作用户既有数据集。

本轮同时评审前两次提交并修复真实 Codex 启动的完整配方摘要问题、服务商配置接线等问题；见 [评审](REVIEW.md) 与 [执行记录](EXECUTION.md)。完整真实 Feature 的 42 个业务节点及两次精确发布完成；现场、恢复过程与 PM 停机状态边界见 [验证报告](VALIDATION.md)。

## 已知语义影响（已接受）

1. wire 与内部结构 breaking，一次性切换合入。
2. `content_hash` 基准变为 hash(refs+data)；本轮只使用新数据集，不处理或改写既有库。
3. `source_views` 仅按当前 `type` 形状识别源引用；不增加旧格式读取或兼容分支。
4. capability/agent 的 `data` 变空对象，文件保留必填 `"data": {}`。
