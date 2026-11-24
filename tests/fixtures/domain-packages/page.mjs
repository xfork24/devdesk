import { createElement as h, useDomain, useState } from "/assets/devdesk-ui.mjs";

export function DomainPage() {
  const { query } = useDomain();
  const [output, setOutput] = useState("");
  return h("form", { onSubmit: async (event) => {
    event.preventDefault();
    const fields = Object.fromEntries(new FormData(event.currentTarget));
    try {
      setOutput(JSON.stringify(await query(fields.handler, fields)));
    } catch (error) {
      setOutput(error.message);
    }
  } },
    h("label", {}, "领域处理器", h("input", { name: "handler", required: true })),
    h("label", {}, "本地仓库路径", h("input", { name: "path" })),
    h("label", {}, "文稿标题", h("input", { name: "title" })),
    h("button", { type: "submit" }, "运行领域查询"),
    h("output", {}, output),
  );
}
