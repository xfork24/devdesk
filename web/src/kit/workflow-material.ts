function materialLabel(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const label = value.trim();
  return label && !/^(?:result|output|材料|产物)$/i.test(label)
    ? label
    : undefined;
}

export function materialTitle(name: unknown, step?: unknown, port?: unknown) {
  const title = materialLabel(name);
  if (title) return title;
  const source = materialLabel(step);
  const field = materialLabel(port);
  if (source && field) return `${source} · ${field}`;
  if (source) return `${source}的交付物`;
  return field || "未命名交付物";
}
