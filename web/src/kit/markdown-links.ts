type MarkdownNode = {
  type: string;
  value?: string;
  url?: string;
  children?: MarkdownNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
};

const chinesePunctuation = /[，。；：！？、（）【】《》“”‘’]/u;
const commit = /(?<![\da-f])([\da-f]{40})(?![\da-f])/giu;

// GFM treats Chinese punctuation following a bare URL as part of its address.
// Use the original source to distinguish those links from explicit Markdown,
// angle-bracket links and code, whose destinations must remain untouched.
export function markdownLinks({
  linkGithubRevisions = false,
}: { linkGithubRevisions?: boolean } = {}) {
  return (tree: MarkdownNode, file: { value: unknown }) => {
    const source = String(file.value);
    const normalize = (parent: MarkdownNode) => {
      parent.children = parent.children?.flatMap((node) => {
        if (node.type === "link" && node.url) {
          const label = node.children?.[0];
          const start = node.position?.start.offset;
          const end = node.position?.end.offset;
          const literal =
            start === undefined || end === undefined
              ? undefined
              : source.slice(start, end);
          if (
            node.children?.length === 1 &&
            label?.type === "text" &&
            typeof literal === "string" &&
            label.value === literal &&
            /^(?:https?:\/\/|www\.)/i.test(literal)
          ) {
            const boundary = literal.search(chinesePunctuation);
            if (boundary > 0) {
              const address = literal.slice(0, boundary);
              return [
                {
                  ...node,
                  url: /^www\./i.test(address) ? `http://${address}` : address,
                  children: [{ type: "text", value: address }],
                },
                { type: "text", value: literal.slice(boundary) },
              ];
            }
          }
          return [node];
        }
        if (node.children) normalize(node);
        return [node];
      });
    };
    normalize(tree);
    if (!linkGithubRevisions) return;

    const repositories = new Map<string, string>();
    const collect = (node: MarkdownNode) => {
      if (node.type === "link" && node.url) {
        const repository = node.url.match(
          /^https?:\/\/github\.com\/([\w.-]+\/[\w.-]+)(?=\/|[?#]|$)/i,
        )?.[1];
        if (repository)
          repositories.set(
            repository.toLowerCase(),
            `https://github.com/${repository}`,
          );
      }
      node.children?.forEach(collect);
    };
    collect(tree);
    // A SHA alone cannot identify its repository. Never guess between repos.
    if (repositories.size !== 1) return;
    const repository = repositories.values().next().value;
    if (!repository) return;
    const revisions = (parent: MarkdownNode) => {
      parent.children = parent.children?.flatMap((node) => {
        if (node.type === "link" || node.type === "linkReference")
          return [node];
        if (
          node.type === "inlineCode" &&
          /^[\da-f]{40}$/i.test(node.value || "")
        )
          return [
            {
              type: "link",
              url: `${repository}/commit/${node.value}`,
              children: [node],
            },
          ];
        if (node.type === "text" && node.value) {
          const nodes: MarkdownNode[] = [];
          let offset = 0;
          for (const match of node.value.matchAll(commit)) {
            if (match.index > offset)
              nodes.push({
                type: "text",
                value: node.value.slice(offset, match.index),
              });
            nodes.push({
              type: "link",
              url: `${repository}/commit/${match[1]}`,
              children: [{ type: "text", value: match[1] }],
            });
            offset = match.index + match[1].length;
          }
          if (offset < node.value.length)
            nodes.push({ type: "text", value: node.value.slice(offset) });
          return nodes.length ? nodes : [node];
        }
        if (node.children) revisions(node);
        return [node];
      });
    };
    revisions(tree);
  };
}
