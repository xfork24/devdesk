//! Frozen test commands are explicit engineering input, independent of a model's success claim.
use crate::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestRunner {
    NodeTest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestRecipe {
    pub runner: TestRunner,
    pub files: Vec<String>,
    pub image: String,
    pub timeout_seconds: u64,
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
            (1..=300).contains(&self.timeout_seconds),
            "测试超时需为 1—300 秒",
        )?;
        require(
            !self.files.is_empty() && self.files.len() <= 100,
            "测试配方需包含 1—100 个确定文件",
        )?;
        let mut seen = std::collections::HashSet::new();
        for file in &self.files {
            require(
                seen.insert(file)
                    && !file.starts_with(['/', '-'])
                    && !file.contains(['\\', '\n', '\r', '\0'])
                    && file
                        .split('/')
                        .all(|s| !s.is_empty() && s != "." && s != ".."),
                "测试文件必须是唯一且不越界的相对路径",
            )?;
        }
        Ok(())
    }
}
