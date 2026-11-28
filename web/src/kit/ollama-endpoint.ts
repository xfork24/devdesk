export function ollamaServiceAddress(value: string) {
  return value.trim().replace(/\/+$/, "").replace(/\/v1$/, "");
}

export function normalizeOllamaServiceAddress(value: string) {
  const address = ollamaServiceAddress(value);
  let url: URL;
  try {
    url = new URL(address);
  } catch {
    throw new Error("Ollama 服务地址无效");
  }
  if (
    address.length > 2048 ||
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== "/"
  ) {
    throw new Error(
      "请填写 HTTP/HTTPS 服务根地址，不包含凭据、查询参数或 API 路径",
    );
  }
  return url.origin;
}

export function modelProviderLabel(provider: { id: string; label: string }) {
  return provider.id === "ollama" ? "本地 Ollama" : provider.label;
}
