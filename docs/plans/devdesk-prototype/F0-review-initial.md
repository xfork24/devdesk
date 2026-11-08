# F0 独立预审

评审者：独立 Agent /root/review_f0。只读静态预审，初次结论：需修改，不以自检代替验收。

发现：当前输入规划门禁缺失；Merge 源未引用确定交付；TDD/来源规则未接实际门禁；退回 Attempt 可被后续 pass 复活；核心生成固定模拟文案；Task 质量 Override 无可达路径；Spec 用途 slot 与规则未实际执行。

修复方向：统一 planning_accepted；Merge.source_delivery 与 Attempt 候选；validate_evidence 硬校验；最终评审决定锁；模拟文案及故障映射移入适配器；断言失败可产生质量待验交付；类型化 SpecSlot 和最小内置规则解释。

复审及实际测试结果另记，初审记录永久保留。
