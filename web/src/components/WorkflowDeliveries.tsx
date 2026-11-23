import { displayText } from "../display-text";
import { useContext, useEffect, useRef, useState } from "react";
import { ConfigurationScope } from "./configuration-scope";
import { configurationHref } from "./configuration-links";
import { loadConfigurationRecord } from "./configuration-state";
import { ConfigurationLink } from "./ConfigurationLink";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import { ConfigurationDisclosure, RevisionSelect } from "./configuration-ui";
import { type Row } from "./rpc-types";
import { DeliverySpecPreview } from "./DeliveryContent";

export function JsonField({
  label,
  value,
  onChange,
  readOnly = false,
}: {
  label: string;
  value: unknown;
  onChange: (value: unknown) => void;
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
      {displayText(label)}
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
      {error ? <span role="alert">{displayText(error)}</span> : null}
    </label>
  );
}

function sourceName(port: string, stepName = "流程输入") {
  return `${stepName} / ${port}`;
}

function sourcePort(document: Row, source?: Row) {
  if (!source?.step || !source.port) return undefined;
  return source.step === "$input"
    ? document.inputs?.[source.port]
    : document.steps.find((item: Row) => item.key === source.step)?.outputs?.[
        source.port
      ];
}

function portName(name: string) {
  return name
    .replace(/[_-]+/g, " ")
    .replace(/\b[a-z]/g, (first) => first.toUpperCase());
}

function materialName(name: string, delivery?: Row, repeated = false) {
  return delivery && !repeated ? delivery.name : portName(name);
}

function materialType(reference?: string, delivery?: Row, fallback = "未设置") {
  return `类型：${delivery ? `${delivery.name} v${delivery.version}` : reference || fallback}`;
}

export function SourceSelect({
  document,
  step,
  value,
  onChange,
  label,
  records = [],
}: {
  document: Row;
  step: Row;
  value: Row;
  onChange: (value: Row) => void;
  label: string;
  records?: Row[];
}) {
  const scope = useContext(ConfigurationScope);
  const port = sourcePort(document, value);
  const delivery =
    records.find((r) => r.id === port?.delivery_ref) ||
    (port?.delivery_ref
      ? {
          id: port.delivery_ref,
          type: "delivery_type",
          name: "来源 Delivery",
        }
      : undefined);
  const sources = [
    ...Object.keys(document.inputs || {}).map((port) => ({
      step: "$input",
      port,
      name: sourceName(port),
    })),
    ...document.steps
      .filter((source: Row) => source.key !== step.key)
      .flatMap((source: Row) =>
        Object.keys(source.outputs || {}).map((port) => ({
          step: source.key,
          port,
          name: sourceName(port, source.name || source.key),
        })),
      ),
  ];
  return (
    <label>
      <span className="configuration-field-title">
        <span>{displayText(label)}</span>
        {delivery ? (
          <ConfigurationLink
            href={configurationHref(delivery, scope?.projectId)}
            label={`查看 ${delivery.name}`}
          />
        ) : null}
      </span>
      <ActionSelect
        aria-label={displayText(label)}
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
            {displayText(source.name)}
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
  onRenameOutput: (previous: string, next: string) => boolean;
}) {
  const scope = useContext(ConfigurationScope);
  const [newPort, setNewPort] = useState<string>();
  const [historicalTypes, setHistoricalTypes] = useState<Row[]>([]);
  const types = records.filter((row) => row.type === "delivery_type");
  const inputs = step.inputs || {},
    outputs = step.outputs || {};
  const inputRefs = Object.entries(inputs).map(([name, value]) => ({
    name,
    ref: sourcePort(document, (value as Row).source)?.delivery_ref as
      string | undefined,
  }));
  const outputRefs = Object.values(outputs).map(
    (value) => (value as Row).delivery_ref,
  );
  const missingTypes = JSON.stringify([
    ...new Set(
      [...inputRefs.map(({ ref }) => ref), ...outputRefs].filter(
        (id) => id && !types.some((row) => row.id === id),
      ),
    ),
  ]);
  useEffect(() => {
    const refs: string[] = JSON.parse(missingTypes);
    if (!scope || !refs.length) return;
    const controller = new AbortController();
    void Promise.allSettled(
      refs.map((id) =>
        loadConfigurationRecord(
          scope.request,
          { id, _index: true },
          scope.projectId,
          controller.signal,
        ),
      ),
    ).then((results) => {
      if (!controller.signal.aborted)
        setHistoricalTypes(
          results.flatMap((result) =>
            result.status === "fulfilled" &&
            result.value.type === "delivery_type"
              ? [result.value]
              : [],
          ),
        );
    });
    return () => controller.abort();
  }, [missingTypes, scope]);
  const typeByRef = new Map(
    [...historicalTypes, ...types].map((row) => [row.id, row]),
  );
  const nextKey = (rows: Row, prefix: string) => {
    let index = 1;
    while (`${prefix}_${index}` in rows) index++;
    return `${prefix}_${index}`;
  };
  return (
    <fieldset className="full-width configuration-disclosure-list">
      <legend>输入与输出</legend>
      {Object.entries(inputs).map(([name, binding]) => {
        const input = binding as Row;
        const reference = inputRefs.find((input) => input.name === name)?.ref;
        const delivery = typeByRef.get(reference);
        const repeated =
          !!reference &&
          inputRefs.filter((input) => input.ref === reference).length > 1;
        const fallback =
          input.type === "constant"
            ? "固定值"
            : input.type === "material"
              ? "材料引用"
              : "未设置";
        const replace = (value: Row) =>
          onChange({ inputs: { ...inputs, [name]: value } });
        return (
          <ConfigurationDisclosure
            key={name}
            id={`workflow-port:input:${name}`}
            name={`输入：${materialName(name, delivery, repeated)}`}
            description={materialType(reference, delivery, fallback)}
            defaultOpen={newPort === `input:${name}`}
          >
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
                  setNewPort(`input:${next}`);
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
                records={
                  historicalTypes.length
                    ? [...records, ...historicalTypes]
                    : records
                }
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
              移除输入 {displayText(name)}
            </ActionButton>
          </ConfigurationDisclosure>
        );
      })}
      <ActionButton
        type="button"
        onClick={() => {
          const name = nextKey(inputs, "input");
          setNewPort(`input:${name}`);
          onChange({
            inputs: {
              ...inputs,
              [name]: {
                type: "source",
                source: { step: "", port: "" },
              },
            },
          });
        }}
      >
        添加输入
      </ActionButton>
      {Object.entries(outputs).map(([name, declaration]) => {
        const output = declaration as Row;
        const delivery = typeByRef.get(output.delivery_ref);
        const repeated =
          !!output.delivery_ref &&
          outputRefs.filter((ref) => ref === output.delivery_ref).length > 1;
        return (
          <ConfigurationDisclosure
            key={name}
            id={`workflow-port:output:${name}`}
            name={`输出：${materialName(name, delivery, repeated)}`}
            description={materialType(output.delivery_ref, delivery)}
            defaultOpen={newPort === `output:${name}`}
          >
            <label>
              输出名称
              <input
                defaultValue={name}
                onBlur={(event) => {
                  const next = event.target.value.trim();
                  if (onRenameOutput(name, next)) setNewPort(`output:${next}`);
                }}
              />
            </label>
            <RevisionSelect
              label={`交付类型 · ${name}`}
              records={types}
              required
              value={output.delivery_ref || ""}
              onChange={(delivery_ref) =>
                onChange({
                  outputs: {
                    ...outputs,
                    [name]: { ...output, delivery_ref },
                  },
                })
              }
            />
            <DeliverySpecPreview
              references={delivery?.data?.spec_refs || []}
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
              移除输出 {displayText(name)}
            </ActionButton>
          </ConfigurationDisclosure>
        );
      })}
      <ActionButton
        type="button"
        onClick={() => {
          const name = nextKey(outputs, "output");
          setNewPort(`output:${name}`);
          onChange({
            outputs: {
              ...outputs,
              [name]: {
                delivery_ref: "",
                required: true,
              },
            },
          });
        }}
      >
        添加输出
      </ActionButton>
    </fieldset>
  );
}
