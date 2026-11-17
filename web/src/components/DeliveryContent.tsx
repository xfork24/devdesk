import { displayText } from "../display-text";
import { useContext, useEffect, useState } from "react";
import { ConfigurationScope } from "./configuration-scope";
import { loadConfigurationRecord } from "./configuration-state";
import { MarkdownPreview } from "./configuration-ui";
import { type Row } from "./rpc-types";
import { LazyDetails, ReadableValue } from "./reading";

export function DeliverySpecPreview({
  references,
  records,
}: {
  references: string[];
  records: Row[];
}) {
  return (
    <div aria-label="交付内容标准">
      {references.map((id) => (
        <ConfigurationSpec
          key={id}
          id={id}
          record={records.find((r) => r.id === id && r.type === "spec")}
        />
      ))}
    </div>
  );
}
function ConfigurationSpec({ id, record }: { id: string; record?: Row }) {
  const scope = useContext(ConfigurationScope);
  const [loaded, setLoaded] = useState<Row>();
  const [error, setError] = useState("");
  const [lifetime] = useState(() => ({
    controller: undefined as AbortController | undefined,
  }));
  useEffect(() => () => lifetime.controller?.abort(), [lifetime]);
  const spec = loaded || record;
  return (
    <details
      onToggle={(event) => {
        if (!event.currentTarget.open) {
          lifetime.controller?.abort();
          lifetime.controller = undefined;
          return;
        }
        if (!scope || !spec?._index || lifetime.controller) return;
        const controller = new AbortController();
        lifetime.controller = controller;
        void loadConfigurationRecord(
          scope.request,
          spec,
          scope.projectId,
          controller.signal,
        )
          .then((row) => {
            if (!controller.signal.aborted) setLoaded(row);
          })
          .catch((error) => {
            if (!controller.signal.aborted) setError(String(error));
          })
          .finally(() => {
            if (lifetime.controller === controller)
              lifetime.controller = undefined;
          });
      }}
    >
      <summary>
        {displayText(spec?.name || "规范不可用")} · {displayText(id)}
      </summary>
      {error && <p role="alert">{displayText(error)}</p>}
      {spec && !spec._index ? (
        <>
          <MarkdownPreview text={spec.data.body || ""} />
          <ul>
            {(spec.data.checks || []).map((c: Row) => (
              <li key={c.key}>
                <strong>{displayText(c.name)}</strong>：
                {displayText(c.instructions)}
                {displayText(c.evidence_required && " · 需定位证据")}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p>{displayText(spec ? "正在加载内容…" : "冻结规范不可用。")}</p>
      )}
    </details>
  );
}

export function ReportBody({ value }: { value: unknown }) {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return <ReadableValue value={value} />;
  const body = value as Row;
  if (typeof body.approve === "boolean" && Array.isArray(body.checks))
    return (
      <>
        <h4>评审结论 · {displayText(body.approve ? "批准" : "未批准")}</h4>
        <ReadableValue value={body.summary} />
        <table>
          <thead>
            <tr>
              <th>检查项</th>
              <th>结果</th>
              <th>证据</th>
            </tr>
          </thead>
          <tbody>
            {body.checks.map((check: Row, i: number) => (
              <tr key={check.key || i}>
                <td>{displayText(check.key || i + 1)}</td>
                <td>
                  {displayText(
                    check.passed === true
                      ? "通过"
                      : check.passed === false
                        ? "未通过"
                        : "未记录",
                  )}
                </td>
                <td>
                  <ReadableValue value={check.evidence} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <ExtraFields value={body} exclude={["approve", "summary", "checks"]} />
      </>
    );
  if (
    Array.isArray(body.tasks) &&
    body.tasks.every(
      (t: Row) =>
        t && typeof t.title === "string" && typeof t.instructions === "string",
    )
  )
    return (
      <>
        <h4>任务规划 · {body.tasks.length} 项</h4>
        <table>
          <thead>
            <tr>
              <th>任务</th>
              <th>依赖</th>
            </tr>
          </thead>
          <tbody>
            {body.tasks.map((task: Row, i: number) => (
              <tr key={task.key || i}>
                <td>
                  {displayText(task.key)} · {displayText(task.title)}
                </td>
                <td>
                  {displayText(
                    Array.isArray(task.dependencies)
                      ? task.dependencies.join("、") || "无"
                      : "未记录",
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {body.tasks.map((task: Row, i: number) => (
          <section key={task.key || i}>
            <h4>{displayText(task.title)}</h4>
            <MarkdownPreview text={task.instructions} />
          </section>
        ))}
        <ExtraFields value={body} exclude={["tasks"]} />
      </>
    );
  const core = [
    "title",
    "summary",
    "body",
    "description",
    "instructions",
    "conclusion",
    "result",
    "status",
    "reason",
    "error",
    "evidence",
  ].filter((k) => body[k] !== undefined);
  return core.length ? (
    <>
      <ReadableValue
        value={Object.fromEntries(core.map((k) => [k, body[k]]))}
      />
      <ExtraFields value={body} exclude={core} />
    </>
  ) : (
    <ReadableValue value={body} />
  );
}
function ExtraFields({ value, exclude }: { value: Row; exclude: string[] }) {
  const rest = Object.fromEntries(
    Object.entries(value).filter(([key]) => !exclude.includes(key)),
  );
  return Object.keys(rest).length ? (
    <LazyDetails summary="补充字段">
      <ReadableValue value={rest} />
    </LazyDetails>
  ) : null;
}
export function DeliveryOutput({
  output,
  definitions,
}: {
  output: Row;
  definitions: Row;
}) {
  const delivery = definitions[output.delivery_ref],
    value = output.value,
    provenance = output.provenance;
  const format = delivery?.data?.format;
  return (
    <div aria-label="交付内容" className="reading-body">
      {format === "diff" ? (
        <>
          <p>比较范围：{displayText(provenance?.comparison || "未记录")}</p>
          <p>
            起始版本：
            <code>{displayText(provenance?.base_revision || "未记录")}</code>
          </p>
          <p>
            结果版本：
            <code>{displayText(provenance?.revision || "未记录")}</code>
          </p>
          <pre aria-label="完整 Diff">
            <code>
              {displayText(
                typeof value === "string"
                  ? value || "无代码差异"
                  : "Diff 内容格式异常，请查看原始 JSON",
              )}
            </code>
          </pre>
        </>
      ) : format === "test_report" && value && typeof value === "object" ? (
        <>
          <h4>
            测试执行结果 · {displayText(value.status || "未记录")}
            {displayText(
              value.expected_failure === true ? " · 符合预期的失败" : "",
            )}
          </h4>
          <ReadableValue
            value={{
              candidate: value.candidate,
              environment: value.environment,
              failure_type: value.failure_type || "未记录失败分类",
            }}
          />
          {value.execution?.command && (
            <>
              <h4>实际命令</h4>
              <pre>
                <code>
                  {displayText(
                    Array.isArray(value.execution.command)
                      ? value.execution.command.join(" ")
                      : String(value.execution.command),
                  )}
                </code>
              </pre>
            </>
          )}
          <ExtraFields
            value={value}
            exclude={[
              "status",
              "expected_failure",
              "candidate",
              "environment",
              "failure_type",
            ]}
          />
        </>
      ) : (
        <ReportBody value={value} />
      )}
      <LazyDetails summary="来源与规范">
        <p>
          {displayText(delivery?.name || output.delivery_ref)} ·{" "}
          <code>{displayText(output.delivery_ref)}</code>
        </p>
        {output.spec_validation && (
          <p>
            结构检查：
            {displayText(
              output.spec_validation.structure_valid ? "通过" : "未通过",
            )}{" "}
            · 语义质量以实际评审为准。
          </p>
        )}
        {provenance && <ReadableValue value={provenance} />}
        <DeliverySpecPreview
          references={delivery?.spec_refs || []}
          records={Object.values(definitions) as Row[]}
        />
      </LazyDetails>
    </div>
  );
}
