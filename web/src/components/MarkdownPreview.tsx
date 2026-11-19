import { createContext, useContext, type ReactNode } from "react";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";
import { displayText } from "../display-text";
import { markdownLinks } from "./markdown-links";

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

function abbreviateUuids() {
  const visit = (container: Container) => {
    for (const node of container.children) {
      if (node.type === "text") node.value = displayText(node.value);
      else if (node.type === "element") {
        for (const key of ["title", "alt"]) {
          if (typeof node.properties[key] === "string")
            node.properties[key] = displayText(node.properties[key]);
        }
        visit(node);
      }
    }
  };
  return visit;
}

export default function MarkdownPreview({
  text,
  trailing,
  linkGithubRevisions = false,
}: {
  text: string;
  trailing?: ReactNode;
  linkGithubRevisions?: boolean;
}) {
  return (
    <TrailingContext.Provider value={trailing}>
      <Markdown
        remarkPlugins={[remarkGfm, [markdownLinks, { linkGithubRevisions }]]}
        rehypePlugins={
          trailing ? [abbreviateUuids, appendTrailing] : [abbreviateUuids]
        }
        components={trailing ? trailingComponents : undefined}
      >
        {text}
      </Markdown>
    </TrailingContext.Provider>
  );
}
