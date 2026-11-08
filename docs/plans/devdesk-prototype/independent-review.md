# 独立评审记录

日期：2026-11-08。以下保存独立Agent的结论与修复跟踪；实现者自检未充当独立评审。

## review_f0：F0/F2/F3/F4

初次退回七项：当前输入规划门禁、Merge源交付固定、TDD/来源实际校验、Review终态、模拟文案位置、质量Override可达性、规范用途与规则执行。

复审补充：旧Merge阻止新源候选；Merge退回后死路；Cancelled可被接纳复活；Reviewer缺少冻结交付输入；Completed可被重复Override重开；在途Merge失效导致整个tick回滚。

均已修复并加入回归：当前planning_accepted、source_delivery、validate_evidence、最终Review决定锁、结构化模拟适配器输出、具名SpecSlot和内置规则、Merge Rejected/Interrupted、可信人类例外、冻结Reviewer与TestRequest输入、终态保护和调度失效隔离。

最终结论：“关闭本评审发现的全部阻塞问题，F3/F4 后端语义审查通过。”结合前轮，F0/F2亦通过。评审者独立执行过30项Rust测试，无失败；后续实现者新增替代Spec和写许可测试，最终总数32。

## review_storage_host：F1/F4

动态探针发现：SSE连接保持时退出挂起、Artifact symlink写入与读取越界、重启Reviewer覆盖已接受Attempt。修复并加入读写symlink、Reviewer重启和跨重启Merge重放测试。

最终结论：“F1 本轮发现的阻塞项已全部关闭。”评审者独立运行HTTP契约脚本，确认鉴权、Host/Origin、幂等、Artifact、SSE退出与重启；存储/host范围F1/F4通过。

## review_web：F5 Linux

初审发现：Merge Runs缺恢复操作、详情截断无分页、弹窗解析/提交错误不可见。复审补充父列表分页未重置Run子页。另建议过期请求保护、焦点与Escape处理、退出状态检查。

全部修复。最终结论：“F5 Web 最终独立评审：Linux 范围通过。”评审者检查新增E2E代码和passed结果记录；此轮为静态复审，计时与实际浏览器运行由实现者报告。

以上为原 Linux 交付时的结论，当时 macOS 仍 pending。本轮实际 macOS 验证另存 [执行报告](macos-validation.md) 和 [独立评审](macos-independent-review.md)，不改写上述历史评语。
