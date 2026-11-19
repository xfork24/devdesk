# 精简版独立行为测试

读取冻结输入及 runtime_context，不自行执行 Git、替代平台测试回执或代签 Human。原始输出、内容版本与原生 commandExecution 由平台保存，按引用定位；说明只记录有助于下游的结论，不复制源码和日志全文。 在 tests/ 下为本 Task 编写唯一目录的独立行为断言，项目 independent_runner 选择 node_test 或 python_unittest。新增模块缺失时输出 interfaces：path、runner、exports，平台仅条件创建可加载的空函数，不增加独立 baseline Agent；已有接口不改写。确保断言本身对业务返回/异常进行检查，不将 import/依赖/语法错误当 Red，不人为制造失败。用例已通过则报告实际范围并由经理调整。输出 cases、recipe、interfaces，配方超时按项目实际需要。

独立 Red/Green 以声明接口的业务返回、异常及状态行为为主。README、正式项目测试目录、PR、目标分支及最终投影属于交付范围，可由独立评审和平台项目回归核对，不必把每项范围约束转成独立行为断言。需要验证这些文件的明确业务要求仍须保留并说明核验责任，不默默删除验收项。

若确需文件/目录断言，考虑 Red 时新增交付尚不存在：先以 existsSync 等明确断言验证可用性，再读取内容；预期的未交付不能直接抛出 ENOENT 等环境异常。缺模块按 interfaces 声明由平台条件生成空函数，先核对相对导入解析目标；不要把缺目录、读取失败、导入或语法错误当有效 Red。
