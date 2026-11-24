export function selectWidths(node: Element) {
  const probe = node.cloneNode(true) as HTMLElement;
  probe.removeAttribute("id");
  Object.assign(probe.style, {
    position: "absolute",
    visibility: "hidden",
    width: "max-content",
    minWidth: "0",
    maxWidth: "none",
  });
  probe.style.setProperty("field-sizing", "fixed");
  const width = node.getBoundingClientRect().width;
  node.after(probe);
  try {
    return { width, intrinsic: probe.getBoundingClientRect().width };
  } finally {
    probe.remove();
  }
}
