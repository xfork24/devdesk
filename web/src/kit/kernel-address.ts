export function normalizeKernelAddress(input: string): string {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    throw new Error("请输入完整的内核地址，例如 http://192.168.1.10:4317");
  }
  if (
    !["http:", "https:"].includes(url.protocol) ||
    !url.hostname ||
    url.port === "0" ||
    url.username ||
    url.password ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      "内核地址仅包含 http(s)://主机:端口，不包含路径、账号或参数。",
    );
  }
  return url.origin;
}
