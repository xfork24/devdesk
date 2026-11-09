import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function MarkdownPreview({ text }: { text: string }) {
  return <Markdown remarkPlugins={[remarkGfm]}>{text}</Markdown>;
}
