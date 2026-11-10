import { ActionButton, ActionSelect } from "./ActionFeedback";
import { RevisionSelect, type Row } from "./configuration-ui";

function ports(step: Row, records: Row[]): string[] {
  switch (step.kind) {
    case "agent.interact":
      return ["proposal"];
    case "proposal.adopt":
      return ["document"];
    case "attempt.execute":
    case "delivery.review":
      return ["delivery"];
    case "merge.prepare":
    case "merge.review":
      return ["merge", "delivery"];
    case "merge.apply":
      return ["merge"];
    case "snapshot.capture":
      return ["snapshot"];
    case "workflow.call":
      return Object.keys(
        records.find((r) => r.id === step.subworkflow_ref)?.data.output_ports ||
          {},
      );
    default:
      return [];
  }
}
function rename(rows: Row, previous: string, next: string): Row {
  if (next !== previous && next in rows) return rows;
  return Object.fromEntries(
    Object.entries(rows).map(([key, value]) => [
      key === previous ? next : key,
      value,
    ]),
  );
}
export function WorkflowDeliveries({
  document,
  step,
  records,
  onChange,
}: {
  document: Row;
  step: Row;
  records: Row[];
  onChange: (patch: Row) => void;
}) {
  const types = records.filter((r) => r.kind === "delivery_type");
  const outputs = step.delivery_outputs || {};
  const inputs = step.delivery_inputs || {};
  const availablePorts = ports(step, records);
  const references = document.steps
    .filter((source: Row) => source.key !== step.key)
    .flatMap((source: Row) =>
      Object.entries(source.delivery_outputs || {}).map(
        ([key, declaration]) => ({
          step: source.key,
          port: key,
          declaration: declaration as Row,
          name: source.name,
        }),
      ),
    );
  return (
    <fieldset className="full-width">
      <legend>Delivery 产出与引用</legend>
      <p>
        在 Delivery 子 tab
        定义交付物类型，在这里选择当前步骤的产出和需要引用的前序交付物。
      </p>
      {Object.entries(outputs).map(([name, value], index) => {
        const declaration = value as Row;
        const patch = (change: Row) =>
          onChange({
            delivery_outputs: {
              ...outputs,
              [name]: { ...declaration, ...change },
            },
          });
        return (
          <fieldset key={index}>
            <legend>产出交付物 {index + 1}</legend>
            <label>
              产出名称
              <input
                value={name}
                onChange={(event) =>
                  onChange({
                    delivery_outputs: rename(outputs, name, event.target.value),
                  })
                }
              />
            </label>
            <RevisionSelect
              label="Delivery 类型修订"
              records={types}
              value={declaration.delivery_ref}
              onChange={(value) => patch({ delivery_ref: value })}
            />
            <label>
              产出来源
              <ActionSelect
                value={declaration.source_port}
                onChange={(event) => patch({ source_port: event.target.value })}
              >
                <option value="">选择本步骤产出</option>
                {availablePorts.map((port) => (
                  <option key={port} value={port}>
                    {port}
                  </option>
                ))}
              </ActionSelect>
            </label>
            <ActionButton
              type="button"
              onClick={() =>
                onChange({
                  delivery_outputs: Object.fromEntries(
                    Object.entries(outputs).filter(([key]) => key !== name),
                  ),
                })
              }
            >
              移除产出
            </ActionButton>
          </fieldset>
        );
      })}
      <ActionButton
        type="button"
        disabled={!availablePorts.length}
        onClick={() => {
          let name = "delivery_1";
          let next = 1;
          while (name in outputs) name = `delivery_${++next}`;
          onChange({
            delivery_outputs: {
              ...outputs,
              [name]: { delivery_ref: "", source_port: availablePorts[0] },
            },
          });
        }}
      >
        添加交付物产出
      </ActionButton>
      {Object.entries(inputs).map(([name, value], index) => {
        const declaration = value as Row;
        return (
          <fieldset key={index}>
            <legend>引用交付物 {index + 1}</legend>
            <label>
              引用名称
              <input
                value={name}
                onChange={(event) =>
                  onChange({
                    delivery_inputs: rename(inputs, name, event.target.value),
                  })
                }
              />
            </label>
            <label>
              前序交付物
              <ActionSelect
                value={JSON.stringify({
                  step: declaration.source?.step,
                  port: declaration.source?.port,
                })}
                onChange={(event) => {
                  if (!event.target.value) return;
                  const source = JSON.parse(event.target.value);
                  const reference = references.find(
                    (r: Row) =>
                      r.step === source.step && r.port === source.port,
                  );
                  onChange({
                    delivery_inputs: {
                      ...inputs,
                      [name]: {
                        source,
                        delivery_ref:
                          reference?.declaration.delivery_ref ||
                          declaration.delivery_ref,
                      },
                    },
                    dependencies: [
                      ...new Set([
                        ...(step.dependencies || []),
                        ...(source.step === "$input" ? [] : [source.step]),
                      ]),
                    ],
                  });
                }}
              >
                <option value="">选择前序阶段的交付物</option>
                {references.map((reference: Row) => (
                  <option
                    key={`${reference.step}:${reference.port}`}
                    value={JSON.stringify({
                      step: reference.step,
                      port: reference.port,
                    })}
                  >
                    {reference.name} / {reference.port} ·{" "}
                    {types.find(
                      (t) => t.id === reference.declaration.delivery_ref,
                    )?.name || reference.declaration.delivery_ref}
                  </option>
                ))}
                {(document.input_ports || []).map((port: string) => (
                  <option
                    key={`input:${port}`}
                    value={JSON.stringify({ step: "$input", port })}
                  >
                    子流程输入 / {port}
                  </option>
                ))}
              </ActionSelect>
            </label>
            {declaration.source?.step === "$input" && (
              <RevisionSelect
                label="引用 Delivery 类型修订"
                records={types}
                value={declaration.delivery_ref}
                onChange={(value) =>
                  onChange({
                    delivery_inputs: {
                      ...inputs,
                      [name]: { ...declaration, delivery_ref: value },
                    },
                  })
                }
              />
            )}
            <ActionButton
              type="button"
              onClick={() =>
                onChange({
                  delivery_inputs: Object.fromEntries(
                    Object.entries(inputs).filter(([key]) => key !== name),
                  ),
                })
              }
            >
              移除引用
            </ActionButton>
          </fieldset>
        );
      })}
      <ActionButton
        type="button"
        onClick={() => {
          let name = "material_1";
          let next = 1;
          while (name in inputs) name = `material_${++next}`;
          onChange({
            delivery_inputs: {
              ...inputs,
              [name]: { delivery_ref: "", source: { step: "", port: "" } },
            },
          });
        }}
      >
        引用前序交付物
      </ActionButton>
    </fieldset>
  );
}
