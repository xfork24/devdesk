//! Frozen test commands are explicit engineering input, independent of a model's success claim.
use crate::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestRunner {
    NodeTest,
    PythonUnittest,
    Command,
}
/// An argv invocation in an exported, credential-free revision. Writable build
/// output belongs in /state or /tmp; the source mount remains read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCommand {
    pub argv: Vec<String>,
    #[serde(default = "test_cwd")]
    pub cwd: String,
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
}
fn test_cwd() -> String {
    ".".into()
}
impl TestCommand {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.argv.is_empty()
                && self.argv.len() <= 100
                && self
                    .argv
                    .iter()
                    .all(|v| !v.contains('\0') && v.len() <= 8192)
                && !self.argv[0].is_empty(),
            "测试入口需要有界的程序及参数",
        )?;
        require(
            self.cwd == "." || test_relative_path(&self.cwd),
            "测试 cwd 必须位于版本内",
        )?;
        require(
            self.environment.len() <= 32
                && self.environment.iter().all(|(k, v)| {
                    !k.is_empty()
                        && k.bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                        && !k.contains("TOKEN")
                        && !k.contains("SECRET")
                        && !k.contains("PASSWORD")
                        && !k.ends_with("API_KEY")
                        && !["PYTHONPATH", "PYTHONHOME", "LD_PRELOAD", "LD_LIBRARY_PATH"]
                            .contains(&k.as_str())
                        && v.len() <= 4096
                        && !v.contains('\0')
                }),
            "测试环境仅允许有界的非凭据配置",
        )
    }
}
pub fn test_relative_path(file: &str) -> bool {
    !file.starts_with(['/', '-'])
        && !file.contains(['\\', '\n', '\r', '\0'])
        && file
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTestConfiguration {
    pub language: String,
    pub independent_runner: TestRunner,
    pub regression: TestRecipe,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestInterface {
    pub path: String,
    pub runner: TestRunner,
    pub exports: Vec<String>,
}
impl TestInterface {
    pub fn source(&self) -> Result<String> {
        require(
            test_relative_path(&self.path)
                && !self.exports.is_empty()
                && self.exports.len() <= 32
                && self.exports.iter().all(|s| {
                    !s.is_empty()
                        && s.bytes().enumerate().all(|(i, b)| {
                            b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit())
                        })
                }),
            "接口基线仅允许相对路径及明确导出标识",
        )?;
        match self.runner {
            TestRunner::NodeTest if self.path.ends_with(".mjs") => Ok(self
                .exports
                .iter()
                .map(|name| format!("export function {name}() {{ return undefined; }}\n"))
                .collect()),
            TestRunner::PythonUnittest if self.path.ends_with(".py") => Ok(self
                .exports
                .iter()
                .map(|name| format!("def {name}(*args, **kwargs):\n    return None\n\n"))
                .collect()),
            _ => Err(err(
                "unsupported",
                "接口基线只支持声明的 ESM/Python 空实现，不接受任意源码",
            )),
        }
    }
}
impl ProjectTestConfiguration {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.language.trim().is_empty() && self.language.len() <= 100,
            "项目测试需声明语言/运行环境",
        )?;
        require(
            self.independent_runner != TestRunner::Command,
            "独立测试需要可区分断言与环境失败的执行器",
        )?;
        self.regression.validate()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestRecipe {
    pub runner: TestRunner,
    pub files: Vec<String>,
    pub image: String,
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<TestCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preparation: Vec<TestCommand>,
}
impl TestRecipe {
    pub fn validate(&self) -> Result<()> {
        require(
            self.image.starts_with("sha256:")
                && self.image.len() == 71
                && self.image[7..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "测试镜像必须是固定 Docker image ID",
        )?;
        require(
            (1..=7200).contains(&self.timeout_seconds),
            "测试超时需为 1—7200 秒",
        )?;
        require(
            !self.files.is_empty() && self.files.len() <= 100,
            "测试配方需包含 1—100 个确定文件",
        )?;
        let mut seen = std::collections::HashSet::new();
        for file in &self.files {
            require(
                seen.insert(file) && test_relative_path(file),
                "测试文件必须是唯一且不越界的相对路径",
            )?;
        }
        require(self.preparation.len() <= 8, "依赖准备入口超出预算")?;
        for command in &self.preparation {
            command.validate()?;
        }
        require(
            (self.runner == TestRunner::Command) == self.command.is_some(),
            "command 执行器必须且只能配置明确入口",
        )?;
        if let Some(command) = &self.command {
            command.validate()?;
        }
        Ok(())
    }
}
