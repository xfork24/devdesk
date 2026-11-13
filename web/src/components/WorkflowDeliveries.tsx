import { useEffect, useRef, useState } from "react";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import { RevisionSelect, type Row } from "./configuration-ui";
import { DeliverySpecPreview } from "./DeliveryContent";

export function JsonField({
  label,
  value,
  onChange,
  readOnly = false,
}: {
  label: string;
  value: unknown;
  onChange: (value: any) => void;
  readOnly?: boolean;
}) {
  const encoded = JSON.stringify(value, null, 2);
  const field = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState(encoded),
    [error, setError] = useState("");
  useEffect(() => {
    setText(encoded);
    setError("");
    field.current?.setCustomValidity("");
  }, [encoded]);
  return (
    <label>
      {label}
      <textarea
        ref={field}
        value={text}
        readOnly={readOnly}
        aria-invalid={!!error}
        onChange={(event) => {
          setText(event.target.value);
          try {
            JSON.parse(event.target.value);
            event.currentTarget.setCustomValidity("");
            setError("");
          } catch {
            event.currentTarget.setCustomValidity("请输入有效的 JSON。");
            setError("请输入有效的 JSON。");
          }
        }}
        onBlur={() => {
          if (readOnly) return;
          try {
            onChange(JSON.parse(text));
          } catch {
            setError("请输入有效的 JSON。");
          }
        }}
      />
      {error ? <span role="alert">{error}</span> : null}
    </label>
  );
}

export function SourceSelect({
  document,
  step,
  value,
  onChange,
  label,
}: {
  document: Row;
  step: Row;
  value: Row;
  onChange: (value: Row) => void;
  label: string;
}) {
  const sources = [
    ...Object.keys(document.inputs || {}).map((port) => ({
      step: "$input",
      port,
      name: `流程输入 / ${port}`,
    })),
    ...document.steps
      .filter((source: Row) => source.key !== step.key)
      .flatMap((source: Row) =>
        Object.keys(source.outputs || {}).map((port) => ({
          step: source.key,
          port,
          name: `${source.name} / ${port}`,
        })),
      ),
  ];
  return (
    <label>
      {label}
      <ActionSelect
        aria-label={label}
        value={value?.step ? `${value.step}:${value.port}` : ""}
        onChange={(event) => {
          const source = sources.find(
            (source) => `${source.step}:${source.port}` === event.target.value,
          );
          if (source) onChange({ step: source.step, port: source.port });
        }}
      >
        <option value="">请选择来源</option>
        {sources.map((source) => (
          <option
            key={`${source.step}:${source.port}`}
            value={`${source.step}:${source.port}`}
          >
            {source.name}
          </option>
        ))}
      </ActionSelect>
    </label>
  );
}

export function WorkflowDeliveries({
  document,
  step,
  records,
  onChange,
  onRenameOutput,
}: {
  document: Row;
  step: Row;
  records: Row[];
  onChange: (patch: Row) => void;
  onRenameOutput: (previous: string, next: string) => void;
}) {
  const types = records.filter((row) => row.type === "delivery_type");
  const inputs = step.inputs || {},
    outputs = step.outputs || {};
  const nextKey = (rows: Row, prefix: string) => {
    let index = 1;
    while (`${prefix}_${index}` in rows) index++;
    return `${prefix}_${index}`;
  };
  return (
    <fieldset className="full-width">
      <legend>输入与输出</legend>
      {Object.entries(inputs).map(([name, binding]) => {
        const input = binding as Row;
        const replace = (value: Row) =>
          onChange({ inputs: { ...inputs, [name]: value } });
        return (
          <fieldset key={name}>
            <legend>输入 · {name}</legend>
            <label>
              输入名称
              <input
                defaultValue={name}
                onBlur={(event) => {
                  event.target.setCustomValidity("");
                  const next = event.target.value.trim();
                  if (next === name) return;
                  if (!/^[A-Za-z0-9_-]{1,80}$/.test(next) || next in inputs) {
                    event.target.setCustomValidity(
                      "输入名称需唯一，使用字母、数字、下划线或连字符。",
                    );
                    event.target.reportValidity();
                    return;
                  }
                  onChange({
                    inputs: Object.fromEntries(
                      Object.entries(inputs).map(([key, value]) => [
                        key === name ? next : key,
                        value,
                      ]),
                    ),
                    access: {
                      ...step.access,
                      workspace_input:
                        step.access?.workspace_input === name
                          ? next
                          : step.access?.workspace_input,
                    },
                  });
                }}
              />
            </label>
            <label>
              输入方式
              <ActionSelect
                value={input.type}
                onChange={(event) =>
                  replace(
                    event.target.value === "constant"
                      ? { type: "constant", value: "" }
                      : { type: "source", source: { step: "", port: "" } },
                  )
                }
              >
                <option value="source">引用材料</option>
                <option value="constant">固定值</option>
              </ActionSelect>
            </label>
            {input.type === "constant" ? (
              <JsonField
                label={`固定值 · ${name}`}
                value={input.value}
                onChange={(value) => replace({ type: "constant", value })}
              />
            ) : (
              <SourceSelect
                document={document}
                step={step}
                value={input.source}
                label={`材料来源 · ${name}`}
                onChange={(source) => {
                  const dependencies =
                    source.step === "$input"
                      ? step.dependencies
                      : [
                          ...new Set([
                            ...(step.dependencies || []),
                            source.step,
                          ]),
                        ];
                  onChange({
                    inputs: { ...inputs, [name]: { type: "source", source } },
                    dependencies,
                  });
                }}
              />
            )}
            <ActionButton
              type="button"
              onClick={() =>
                onChange({
                  inputs: Object.fromEntries(
                    Object.entries(inputs).filter(([key]) => key !== name),
                  ),
                })
              }
            >
              移除输入 {name}
            </ActionButton>
          </fieldset>
        );
      })}
      <ActionButton
        type="button"
        onClick={() =>
          onChange({
            inputs: {
              ...inputs,
              [nextKey(inputs, "input")]: {
                type: "source",
                source: { step: "", port: "" },
              },
            },
          })
        }
      >
        添加输入
      </ActionButton>
      {Object.entries(outputs).map(([name, declaration]) => {
        const output = declaration as Row;
        return (
          <fieldset key={name}>
            <legend>输出 · {name}</legend>
            <label>
              输出名称
              <input
                defaultValue={name}
                onBlur={(event) =>
                  onRenameOutput(name, event.target.value.trim())
                }
              />
            </label>
            <RevisionSelect
              label={`交付类型 · ${name}`}
              records={types}
              required
              value={output.delivery_ref || ""}
              onChange={(delivery_ref) =>
                onChange({
                  outputs: { ...outputs, [name]: { ...output, delivery_ref } },
                })
              }
            />
            <DeliverySpecPreview
              references={
                types.find((row) => row.id === output.delivery_ref)?.data
                  .spec_refs || []
              }
              records={records}
            />
            <label>
              <input
                type="checkbox"
                checked={output.required !== false}
                onChange={(event) =>
                  onChange({
                    outputs: {
                      ...outputs,
                      [name]: { ...output, required: event.target.checked },
                    },
                  })
                }
              />
              必须产出
            </label>
            <label>
              <input
                type="checkbox"
                checked={!!output.content_checks}
                onChange={(event) =>
                  onChange({
                    outputs: {
                      ...outputs,
                      [name]: {
                        ...output,
                        content_checks: event.target.checked,
                      },
                    },
                  })
                }
              />
              按 Spec 的内容检查项提交判断报告
            </label>
            <ActionButton
              type="button"
              onClick={() =>
                onChange({
                  outputs: Object.fromEntries(
                    Object.entries(outputs).filter(([key]) => key !== name),
                  ),
                })
              }
            >
              移除输出 {name}
            </ActionButton>
          </fieldset>
        );
      })}
      <ActionButton
        type="button"
        onClick={() =>
          onChange({
            outputs: {
              ...outputs,
              [nextKey(outputs, "output")]: {
                delivery_ref: "",
                required: true,
              },
            },
          })
        }
      >
        添加输出
      </ActionButton>
    </fieldset>
  );
}
