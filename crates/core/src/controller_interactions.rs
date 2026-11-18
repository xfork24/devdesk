//! Human statements and immutable interaction evidence.
use crate::*;
pub(crate) fn explicit_approval(input: &str, interaction: &Value) -> bool {
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    let clauses: Vec<_> = input
        .split(['，', ',', '。', '！', '!', '\n', ';', '；'])
        .map(normalize)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(first) = clauses.first() else {
        return false;
    };
    if clauses.iter().skip(1).any(|s| {
        ![
            "继续",
            "请继续",
            "继续执行",
            "继续流程",
            "继续后续流程",
            "继续后续步骤",
            "继续执行后续步骤",
            "继续执行后续流程",
            "继续设计",
        ]
        .contains(&s.as_str())
    }) {
        return false;
    }
    let mut subjects = vec![
        "当前事项".to_owned(),
        "当前版本".to_owned(),
        "当前结果".to_owned(),
        "此版本结果".to_owned(),
    ];
    if let Some(title) = interaction["title"].as_str() {
        subjects.push(normalize(title.trim_start_matches("确认")));
    }
    if interaction.to_string().to_lowercase().contains("prd") {
        subjects.push("prd".into());
    }
    ["确认", "批准", "同意", "通过", "approve", "approved"].contains(&first.as_str())
        || ["确认", "批准", "同意", "通过"].iter().any(|verb| {
            subjects
                .iter()
                .any(|subject| !subject.is_empty() && *first == format!("{verb}{subject}"))
        })
}
