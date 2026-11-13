//! Bounded structural checks and evidence locations. No environment or execution claims.
use crate::*;
use std::collections::BTreeSet;
pub const SPEC_CHECKER_ID: &str = "spec-primitives/v1";
pub const SPEC_VALUE_LIMIT: usize = 8 * 1024 * 1024;
pub const EVIDENCE_QUOTE_INSTRUCTIONS: &str = "Optional. If present, quote must equal the entire selected text, not merely occur within it. An empty JSON pointer selects the whole frozen input. For a short quotation, select its exact lines/characters range, or omit quote. Do not put a sentence quote on a whole-document pointer.";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineAssertion {
    TextLength { min: usize, max: usize },
    RequiredSections { headings: Vec<String> },
    RequiredPointers { pointers: Vec<String> },
}
impl MachineAssertion {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::TextLength { min, max } => require(
                min <= max && *max <= SPEC_VALUE_LIMIT,
                "文字长度检查范围无效",
            ),
            Self::RequiredSections { headings } => require(
                !headings.is_empty()
                    && headings.len() <= 64
                    && headings
                        .iter()
                        .all(|h| !h.trim().is_empty() && h.len() <= 256)
                    && headings.iter().collect::<BTreeSet<_>>().len() == headings.len(),
                "章节检查需要 1—64 个唯一标题",
            ),
            Self::RequiredPointers { pointers } => require(
                !pointers.is_empty()
                    && pointers.len() <= 64
                    && pointers.iter().all(|p| valid_pointer(p))
                    && pointers.iter().collect::<BTreeSet<_>>().len() == pointers.len(),
                "JSON 路径检查无效",
            ),
        }
    }
    pub fn evaluate(&self, value: &Value) -> Result<bool> {
        self.validate()?;
        require(
            serde_json::to_vec(value)
                .map_err(|e| err("invalid_value", e.to_string()))?
                .len()
                <= SPEC_VALUE_LIMIT,
            "规范检查内容超过 8 MiB",
        )?;
        Ok(match self {
            Self::TextLength { min, max } => value.as_str().is_some_and(|s| {
                let n = s.chars().count();
                n >= *min && n <= *max
            }),
            Self::RequiredPointers { pointers } => {
                pointers.iter().all(|p| value.pointer(p).is_some())
            }
            Self::RequiredSections { headings } => value.as_str().is_some_and(|text| {
                // Fenced code examples do not satisfy a document heading requirement.
                let mut fence: Option<(char, usize)> = None;
                let mut actual = BTreeSet::new();
                for raw in text.lines() {
                    let indent = raw.chars().take_while(|c| *c == ' ').count();
                    if indent > 3 || raw.starts_with('\t') {
                        continue;
                    }
                    let line = raw.trim_start();
                    let marker = line.chars().next().filter(|c| *c == '`' || *c == '~');
                    if let Some(marker) = marker {
                        let count = line.chars().take_while(|c| *c == marker).count();
                        if count >= 3 {
                            if let Some((opened, length)) = fence {
                                if marker == opened
                                    && count >= length
                                    && line[count..].trim().is_empty()
                                {
                                    fence = None;
                                }
                            } else {
                                fence = Some((marker, count));
                            }
                            continue;
                        }
                    }
                    if fence.is_some() {
                        continue;
                    }
                    let title = line.trim_start_matches('#');
                    let depth = line.len() - title.len();
                    if (1..=6).contains(&depth) && title.starts_with(' ') {
                        actual.insert(title.trim().trim_end_matches('#').trim().to_owned());
                    }
                }
                headings.iter().all(|heading| actual.contains(heading))
            }),
        })
    }
}
fn valid_pointer(pointer: &str) -> bool {
    pointer.len() <= 1024
        && (pointer.is_empty() || pointer.starts_with('/'))
        && !pointer.contains('\0')
        && pointer
            .as_bytes()
            .windows(2)
            .all(|w| w[0] != b'~' || [b'0', b'1'].contains(&w[1]))
        && !pointer.ends_with('~')
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceLocator {
    JsonPointer { pointer: String },
    Lines { start: usize, end: usize },
    Characters { start: usize, end: usize },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLocation {
    pub source_ref: Id,
    pub value_digest: String,
    pub locator: EvidenceLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceTarget {
    pub source_ref: Id,
    pub value_digest: String,
    pub value: Value,
}
impl EvidenceLocation {
    /// The same bounded primitive shape must be visible to producers before admission.
    pub fn json_schema() -> Value {
        serde_json::json!({"type":"object","required":["source_ref","value_digest","locator"],"additionalProperties":false,
        "properties":{"source_ref":{"type":"string","minLength":1},"value_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},"quote":{"type":"string","maxLength":4096,"description":EVIDENCE_QUOTE_INSTRUCTIONS},
            "locator":{"oneOf":[
                {"type":"object","required":["type","pointer"],"properties":{"type":{"const":"json_pointer"},"pointer":{"type":"string","maxLength":1024}},"additionalProperties":false},
                {"type":"object","required":["type","start","end"],"properties":{"type":{"const":"lines"},"start":{"type":"integer","minimum":1},"end":{"type":"integer","minimum":1}},"additionalProperties":false},
                {"type":"object","required":["type","start","end"],"properties":{"type":{"const":"characters"},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":1}},"additionalProperties":false}
            ]}}})
    }
    pub fn verify<'a>(&self, authorized: &'a [EvidenceTarget]) -> Result<&'a EvidenceTarget> {
        let target = authorized
            .iter()
            .find(|t| t.source_ref == self.source_ref && t.value_digest == self.value_digest)
            .ok_or_else(|| err("evidence_scope", "证据不属于本次冻结输入或内容摘要不符"))?;
        require(
            target.value_digest == configuration_hash(&target.value),
            "冻结证据内容摘要不一致",
        )?;
        let selected = match &self.locator {
            EvidenceLocator::JsonPointer { pointer } => {
                require(valid_pointer(pointer), "证据 JSON 路径无效")?;
                let value = target
                    .value
                    .pointer(pointer)
                    .ok_or_else(|| err("evidence_location", "证据 JSON 路径不存在"))?;
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            }
            EvidenceLocator::Lines { start, end } => {
                let text = target
                    .value
                    .as_str()
                    .ok_or_else(|| err("evidence_location", "行定位只支持文本"))?;
                require(*start > 0 && start <= end, "证据行范围无效")?;
                let lines: Vec<_> = text.lines().collect();
                require(*end <= lines.len(), "证据行范围越界")?;
                lines[start - 1..*end].join("\n")
            }
            EvidenceLocator::Characters { start, end } => {
                let text = target
                    .value
                    .as_str()
                    .ok_or_else(|| err("evidence_location", "字符定位只支持文本"))?;
                require(
                    start < end && *end <= text.chars().count(),
                    "证据字符范围越界",
                )?;
                text.chars().skip(*start).take(end - start).collect()
            }
        };
        if let Some(quote) = &self.quote {
            require(
                quote.len() <= 4096 && *quote == selected,
                "证据引用文字与实际位置不符",
            )?;
        }
        Ok(target)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_checks_ignore_code_fences_and_use_configuration_titles() {
        let check = MachineAssertion::RequiredSections {
            headings: vec!["自定义结论".into(), "依据".into()],
        };
        assert!(
            check
                .evaluate(&serde_json::json!("# 自定义结论\ntext\n## 依据\nproof"))
                .unwrap()
        );
        for text in [
            "```md\n# 自定义结论\n## 依据\n```",
            "````\n~~~\n# 自定义结论\n## 依据\n````",
            "    # 自定义结论\n    ## 依据",
        ] {
            assert!(!check.evaluate(&serde_json::json!(text)).unwrap());
        }
    }
    #[test]
    fn unicode_length_and_json_pointer_primitives_have_exact_boundaries() {
        let check = MachineAssertion::TextLength { min: 2, max: 2 };
        assert!(check.evaluate(&serde_json::json!("中文")).unwrap());
        assert!(!check.evaluate(&serde_json::json!("中文a")).unwrap());
        let check = MachineAssertion::RequiredPointers {
            pointers: vec!["/a~1b/0".into()],
        };
        assert!(check.evaluate(&serde_json::json!({"a/b":[null]})).unwrap());
        assert!(!check.evaluate(&serde_json::json!({"a/b":[]})).unwrap());
        assert!(
            MachineAssertion::RequiredPointers {
                pointers: vec!["/bad~x".into()]
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn evidence_requires_frozen_source_digest_valid_location_and_exact_quote() {
        let value = serde_json::json!("第一行\n第二行证据");
        let target = EvidenceTarget {
            source_ref: "input:actual".into(),
            value_digest: configuration_hash(&value),
            value,
        };
        let mut evidence = EvidenceLocation {
            source_ref: target.source_ref.clone(),
            value_digest: target.value_digest.clone(),
            locator: EvidenceLocator::Lines { start: 2, end: 2 },
            quote: Some("第二行证据".into()),
        };
        assert!(evidence.verify(std::slice::from_ref(&target)).is_ok());
        evidence.locator = EvidenceLocator::JsonPointer { pointer: "".into() };
        // A real substring is insufficient when the locator selects the whole document.
        assert!(evidence.verify(std::slice::from_ref(&target)).is_err());
        evidence.quote = None;
        assert!(evidence.verify(std::slice::from_ref(&target)).is_ok());
        evidence.locator = EvidenceLocator::Characters { start: 4, end: 9 };
        evidence.quote = Some("第二行证据".into());
        assert!(evidence.verify(std::slice::from_ref(&target)).is_ok());
        evidence.locator = EvidenceLocator::Lines { start: 2, end: 2 };
        evidence.quote = Some("虚构证据".into());
        assert!(evidence.verify(std::slice::from_ref(&target)).is_err());
        evidence.quote = None;
        evidence.locator = EvidenceLocator::Characters {
            start: 99,
            end: 100,
        };
        assert!(evidence.verify(std::slice::from_ref(&target)).is_err());
        evidence.locator = EvidenceLocator::JsonPointer { pointer: "".into() };
        evidence.value_digest = "a".repeat(64);
        assert!(evidence.verify(std::slice::from_ref(&target)).is_err());
        evidence.value_digest = target.value_digest.clone();
        evidence.source_ref = "another-input".into();
        assert!(evidence.verify(&[target]).is_err());
    }
    #[test]
    fn model_claims_cannot_override_machine_checks_or_replace_required_locations() {
        let expected:Vec<ContentCheckDefinition>=serde_json::from_value(serde_json::json!([{"key":"length","name":"Length","instructions":"Check actual input","assertion":{"type":"text_length","min":10,"max":100}}])).unwrap();
        let value = serde_json::json!("short");
        let target = EvidenceTarget {
            source_ref: "frozen".into(),
            value_digest: configuration_hash(&value),
            value,
        };
        let mut report:ContentCheckReport=serde_json::from_value(serde_json::json!({"approve":true,"summary":"Claim","checks":[{"key":"length","passed":true,"evidence":"a nonempty string"}]})).unwrap();
        assert!(
            report
                .validate_evidence(&expected, std::slice::from_ref(&target))
                .is_err()
        );
        report.checks[0].locations.push(EvidenceLocation {
            source_ref: target.source_ref.clone(),
            value_digest: target.value_digest.clone(),
            locator: EvidenceLocator::JsonPointer { pointer: "".into() },
            quote: None,
        });
        assert!(
            report
                .validate_evidence(&expected, std::slice::from_ref(&target))
                .is_err()
        );
        report.checks[0].passed = false;
        report.approve = false;
        assert!(report.validate_evidence(&expected, &[target]).is_ok());
    }
}
