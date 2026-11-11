import { ActionButton } from "./ActionFeedback";
import { JsonField } from "./WorkflowDeliveries";
import { MarkdownField, type Row } from "./configuration-ui";

export function WorkflowSpecifications({
  kind,
  data,
  onChange,
}: {
  kind: string;
  data: Row;
  records: Row[];
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
  if (kind === "spec")
    return (
      <div className="full-width">
        <p>
          内容要求由下面的检查项定义，可由绑定的 Agent 按当前 Skill 和材料评审。
        </p>
        <p className="muted">
          文档与规范的关联在 Delivery 中选择；Standard
          保存流程与规范的不可变快照。
        </p>
        {(data.checks || []).map((check: Row, index: number) => {
          const patch = (patch: Row) =>
            onChange({
              checks: data.checks.map((value: Row, i: number) =>
                i === index ? { ...value, ...patch } : value,
              ),
            });
          return (
            <fieldset key={index}>
              <legend>检查项 {index + 1}</legend>
              <label>
                检查标识
                <input
                  required
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
              <ActionButton
                type="button"
                onClick={() =>
                  onChange({
                    checks: data.checks.filter(
                      (_: Row, i: number) => i !== index,
                    ),
                  })
                }
              >
                删除检查项
              </ActionButton>
            </fieldset>
          );
        })}
        <ActionButton
          type="button"
          onClick={() =>
            onChange({
              checks: [
                ...(data.checks || []),
                {
                  key: `check_${crypto.randomUUID().slice(0, 8)}`,
                  name: "",
                  instructions: "",
                },
              ],
            })
          }
        >
          添加检查项
        </ActionButton>
      </div>
    );
  return null;
}
