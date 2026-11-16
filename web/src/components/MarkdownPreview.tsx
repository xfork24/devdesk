import { createContext, useContext, type ReactNode } from "react";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";

type Element = NonNullable<ExtraProps["node"]>;
type Container = Pick<Element, "children">;
const TrailingContext = createContext<ReactNode>(null);
const trailingComponents: Components = {
  span: function MarkdownSpan({ node, children, ...props }) {
    const trailing = useContext(TrailingContext);
    return node?.properties.dataMessageTrailing ? (
      <span className="markdown-trailing">{trailing}</span>
    ) : (
      <span {...props}>{children}</span>
    );
  },
};

function trailingContainer(container: Container): Container {
  const last = [...container.children]
    .reverse()
    .find((node) => node.type === "element");
  if (last?.type !== "element") return container;
  if (["p", "h1", "h2", "h3", "h4", "h5", "h6"].includes(last.tagName))
    return last;
  if (["ul", "ol", "li", "blockquote"].includes(last.tagName))
    return trailingContainer(last);
  return container;
}

function appendTrailing() {
  return (tree: Container) => {
    trailingContainer(tree).children.push({
      type: "element",
      tagName: "span",
      properties: { dataMessageTrailing: true },
      children: [],
    });
  };
}

export default function MarkdownPreview({
  text,
  trailing,
}: {
  text: string;
  trailing?: ReactNode;
}) {
  return (
    <TrailingContext.Provider value={trailing}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={trailing ? [appendTrailing] : []}
        components={trailing ? trailingComponents : undefined}
      >
        {text}
      </Markdown>
    </TrailingContext.Provider>
  );
}
