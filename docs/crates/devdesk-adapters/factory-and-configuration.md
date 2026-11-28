# 适配器工厂与配置装载

这个专题从“程序怎样得到可用配置和外部实现”开始。输入是数据集路径和已发布资源，输出是 GlobalStore、配置目录及供 Host 注入的执行适配器。

## 程序装配

[lib.rs](../../../crates/adapters/src/lib.rs) 的 `execution_adapters` 构造 Harness、Git、Tests 和 PullRequests 实现。Host 再把它们与工程存储和产物存储传入 Engine。这是具体实现进入应用层的明确入口。

[configuration_backend.rs](../../../crates/adapters/src/configuration_backend.rs) 实现 core 所需的配置后端检查。它处理具体后端能力，发布是否成立仍由应用用例决定。

## 出厂配置怎样进入程序

[build.rs](../../../crates/adapters/build.rs) 从 `kernel/core/` 读取发布输入，[factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) 装载内嵌资源并形成配置集合。manifest 是发布清单，不能将目录里未列入清单的文件自动当作有效默认。

修改核心配置后重新构建 Host。领域配置由 [domain_packages.rs](../../../crates/adapters/src/domain_packages.rs) 从 ZIP 读取，运行中按活动代次加载。

## GlobalStore 与目录视图

[catalog.rs](../../../crates/adapters/src/catalog.rs) 的 GlobalStore 打开数据集、取得独占锁、检查布局和数据库版本，再提供配置及全局设置存储。

[configuration_cache.rs](../../../crates/adapters/src/configuration_cache.rs) 提供版本化目录缓存，[source_views.rs](../../../crates/adapters/src/source_views.rs) 整理源读取视图。它们可以减少重复解码，但不能成为第二套可修改的源定义存储。

[configuration_reset.rs](../../../crates/adapters/src/configuration_reset.rs) 支持模板重置的存储动作。重置需要保留原目录世代检查，不能让旧编辑页面把草稿写进新的目录上下文。

## 修改与验证

新增配置资源需更新 manifest、根引用、Schema 和默认采用关系。新增适配器需更新工厂和能力声明，并确认 Host 使用了它，不能只增加一个无人实例化的实现。

相关验证：[configuration_catalog.rs](../../../crates/adapters/tests/configuration_catalog.rs)、[catalog_commit_cache.rs](../../../crates/adapters/tests/catalog_commit_cache.rs)、[dataset_preflight.rs](../../../crates/adapters/tests/dataset_preflight.rs)。
