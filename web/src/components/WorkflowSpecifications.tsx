import { useState } from "react";
import { displayText } from "../display-text";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import { JsonField } from "./WorkflowDeliveries";
import { ConfigurationDisclosure, MarkdownField } from "./configuration-ui";
import { type Row } from "./rpc-types";

export function WorkflowSpecifications({
  kind,
  data,
  onChange,
}: {
  kind: string;
  data: Row;
  onChange: (value: Row) => void;
}) {
  if (kind === "delivery_type")
    return (
      <div className="full-width">
        <label>
          <input
            type="checkbox"
            checked={
              data.content_schema !== null && data.content_schema !== undefined
            }
            onChange={(event) =>
              onChange({
                content_schema: event.target.checked
                  ? { type: "object" }
                  : null,
              })
            }
          />
          校验结构化内容
        </label>
        {data.content_schema !== null && data.content_schema !== undefined ? (
          <JsonField
            label="内容 JSON Schema"
            value={data.content_schema}
            onChange={(content_schema) => onChange({ content_schema })}
          />
        ) : null}
      </div>
    );
  return kind === "spec" ? (
    <SpecChecks data={data} onChange={onChange} />
  ) : null;
}

function SpecChecks({
  data,
  onChange,
}: {
  data: Row;
  onChange: (value: Row) => void;
}) {
  const checks: Row[] = data.checks || [];
  const [rows, setRows] = useState(() =>
    checks.map((check, i) => ({
      id: String(i),
      legacy: !!check.key && !check.key.startsWith("check-"),
    })),
  );
  const [newRow, setNewRow] = useState<string>();
  return (
    <div className="full-width">
      <p>
        内容要求由下面的检查项定义，可由绑定的 Agent 按当前 Skill 和材料评审。
      </p>
      <p className="muted">
        文档与规范的关联在 Delivery 中选择；Standard
        保存流程与规范的不可变快照。
      </p>
      <div className="configuration-disclosure-list">
        {checks.map((check, index) => {
          const patch = (patch: Row) =>
            onChange({
              checks: checks.map((value, i) =>
                i === index ? { ...value, ...patch } : value,
              ),
            });
          return (
            <ConfigurationDisclosure
              key={rows[index].id}
              name={check.name || "未填写名称"}
              defaultOpen={newRow === rows[index].id}
            >
              <label>
                检查标识
                <input
                  required
                  pattern={
                    rows[index].legacy
                      ? undefined
                      : "check-[a-z][a-z0-9]*(?:-[a-z0-9]+)*"
                  }
                  title="例如 check-scope-and-version"
                  value={check.key}
                  onChange={(event) => patch({ key: event.target.value })}
                />
              </label>
              <label>
                检查名称
                <input
                  required
                  value={check.name}
                  onChange={(event) => patch({ name: event.target.value })}
                />
              </label>
              <MarkdownField
                label={`检查要求 ${index + 1}`}
                value={check.instructions}
                required
                onChange={(instructions) => patch({ instructions })}
              />
              <label>
                自动检查原语
                <ActionSelect
                  aria-label={displayText(`自动检查原语 ${index + 1}`)}
                  value={check.assertion?.type || "semantic"}
                  onChange={(event) => {
                    const type = event.target.value;
                    const assertion =
                      type === "semantic"
                        ? undefined
                        : type === "text_length"
                          ? { type, min: 0, max: 200000 }
                          : type === "required_sections"
                            ? { type, headings: ["结论", "依据"] }
                            : { type, pointers: ["/summary"] };
                    patch({ assertion });
                  }}
                >
                  <option value="semantic">语义判断</option>
                  <option value="text_length">文字长度</option>
                  <option value="required_sections">必需章节</option>
                  <option value="required_pointers">必需 JSON 路径</option>
                </ActionSelect>
              </label>
              {check.assertion && (
                <JsonField
                  label={`自动检查参数 ${index + 1}`}
                  value={check.assertion}
                  onChange={(assertion) => patch({ assertion })}
                />
              )}
              <label>
                <input
                  type="checkbox"
                  checked={check.evidence_required || false}
                  onChange={(e) =>
                    patch({ evidence_required: e.target.checked })
                  }
                />
                评审必须提供可定位的冻结输入证据
              </label>
              <ActionButton
                type="button"
                onClick={() => {
                  setRows(rows.filter((_, i) => i !== index));
                  onChange({
                    checks: checks.filter((_, i) => i !== index),
                  });
                }}
              >
                删除检查项
              </ActionButton>
            </ConfigurationDisclosure>
          );
        })}
        <ActionButton
          type="button"
          onClick={() => {
            const id = crypto.randomUUID();
            setRows([...rows.slice(0, checks.length), { id, legacy: false }]);
            setNewRow(id);
            onChange({
              checks: [
                ...checks,
                { key: "check-", name: "", instructions: "" },
              ],
            });
          }}
        >
          添加检查项
        </ActionButton>
      </div>
    </div>
  );
}
