import { MarkdownPreview, type Row } from "./configuration-ui";

export function DeliverySpecPreview({
  references,
  records,
}: {
  references: string[];
  records: Row[];
}) {
  return (
    <div aria-label="交付内容标准">
      {references.map((id) => {
        const spec = records.find((r) => r.id === id && r.type === "spec");
        return (
          <details key={id}>
            <summary>
              {spec?.name || "规范不可用"} · {id}
            </summary>
            {spec ? (
              <>
                <MarkdownPreview text={spec.data.body || ""} />
                <ul>
                  {(spec.data.checks || []).map((check: Row) => (
                    <li key={check.key}>
                      <strong>{check.name}</strong>：{check.instructions}
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p role="alert">请关联可用的内容 Spec。</p>
            )}
          </details>
        );
      })}
    </div>
  );
}

export function DeliveryOutput({
  output,
  definitions,
}: {
  output: Row;
  definitions: Row;
}) {
  const delivery = definitions[output.delivery_ref];
  const value = output.value;
  const provenance = output.provenance;
  const format = delivery?.data?.format;
  const records = Object.values(definitions) as Row[];
  return (
    <div aria-label="交付内容">
      <p>
        <strong>{delivery?.name || output.delivery_ref}</strong> ·{" "}
        {output.delivery_ref}
      </p>
      <DeliverySpecPreview
        references={delivery?.spec_refs || []}
        records={records}
      />
      {provenance ? (
        <p>
          来源：{provenance.kind} {provenance.tool || ""} ·{" "}
          {provenance.call_id || provenance.turn_id || ""}
        </p>
      ) : null}
      {format === "diff" ? (
        <>
          <p>比较范围：{provenance?.comparison || "未记录"}</p>
          <p>
            起始版本：<code>{provenance?.base_revision || "未记录"}</code>
          </p>
          <p>
            结果版本：<code>{provenance?.revision || "未记录"}</code>
          </p>
          <p>
            内容摘要：<code>{provenance?.value_digest || "未记录"}</code>
          </p>
          <pre aria-label="完整 Diff">
            <code>{value || "无代码差异"}</code>
          </pre>
        </>
      ) : format === "test_report" ? (
        <>
          <dl>
            <dt>执行结果</dt>
            <dd>
              {value.status}
              {value.expected_failure ? " · 符合预期的断言失败" : ""}
            </dd>
            <dt>被测版本</dt>
            <dd>
              <code>{value.candidate}</code>
            </dd>
            <dt>环境</dt>
            <dd>{value.environment}</dd>
            <dt>失败分类</dt>
            <dd>{value.failure_type || "无"}</dd>
          </dl>
          <pre>{value.execution?.command}</pre>
          <details>
            <summary>完整测试报告与原始输出</summary>
            <pre>{JSON.stringify(value, null, 2)}</pre>
          </details>
        </>
      ) : Array.isArray(value) ? (
        <>
          <p>共 {value.length} 项交付</p>
          {value.map((item: Row, index: number) => (
            <details key={item.operation_id || index}>
              <summary>
                Task{" "}
                {item.inputs?.task?.key || item.inputs?.task?.id || index + 1}
              </summary>
              <pre>{JSON.stringify(item, null, 2)}</pre>
            </details>
          ))}
        </>
      ) : typeof value === "string" ? (
        <MarkdownPreview text={value} />
      ) : (
        <pre>{JSON.stringify(value, null, 2)}</pre>
      )}
    </div>
  );
}
