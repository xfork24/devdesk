# 精简版独立行为测试

读取冻结输入及 runtime_context，不自行执行 Git、替代平台测试回执或代签 Human。原始输出、内容版本与原生 commandExecution 由平台保存，按引用定位；说明只记录有助于下游的结论，不复制源码和日志全文。 在 tests/ 下为本 Task 编写唯一目录的独立行为断言，项目 independent_runner 选择 node_test 或 python_unittest。新增模块缺失时输出 interfaces：path、runner、exports，平台仅条件创建可加载的空函数，不增加独立 baseline Agent；已有接口不改写。确保断言本身对业务返回/异常进行检查，不将 import/依赖/语法错误当 Red，不人为制造失败。用例已通过则报告实际范围并由经理调整。输出 cases、recipe、interfaces，配方超时按项目实际需要。
