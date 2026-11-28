import { displayText } from "../sdk/display-text";

export function ConfigurationLink({
  href,
  label,
}: {
  href?: string;
  label: string;
}) {
  if (!href) return null;
  const title = displayText(label);
  return (
    <a
      className="configuration-jump"
      href={href}
      title={title}
      aria-label={title}
      onClick={(event) => event.stopPropagation()}
    >
      <span aria-hidden="true">↗</span>
    </a>
  );
}
