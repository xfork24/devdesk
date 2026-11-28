import {
  Children,
  createContext,
  isValidElement,
  type OptionHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";

export const HistoricalDiscussionAgents = createContext<
  ReadonlySet<string> | undefined
>(undefined);

export function discussionRoleOptions(
  children: ReactNode,
  selected: SelectHTMLAttributes<HTMLSelectElement>["value"],
  historical?: ReadonlySet<string>,
) {
  const labels = new Map<string, string>();
  const revisions = new Map<ReactNode, string>();
  const options = Children.toArray(children).filter((option) => {
    if (
      !isValidElement<OptionHTMLAttributes<HTMLOptionElement>>(option) ||
      option.type !== "option" ||
      typeof option.props.value !== "string"
    )
      return true;
    const revision = option.props.value;
    if (revision !== selected && historical?.has(revision)) return false;
    const label = Children.toArray(option.props.children);
    if (
      label.every(
        (part) => typeof part === "string" || typeof part === "number",
      )
    ) {
      labels.set(revision, label.join(""));
      revisions.set(option, revision);
    }
    return true;
  });
  // A historical session can select the predecessor of a packaged revision.
  // Show its exact selection when the two options would have the same label.
  const legacy = "builtin-agent-";
  const packaged = "software-development.agent-";
  return options.filter((option) => {
    const revision = revisions.get(option);
    if (!revision) return true;
    if (revision.startsWith(legacy)) {
      const equivalent = packaged + revision.slice(legacy.length);
      return (
        selected === revision || labels.get(revision) !== labels.get(equivalent)
      );
    }
    const equivalent = legacy + revision.slice(packaged.length);
    return (
      !revision.startsWith(packaged) ||
      selected !== equivalent ||
      labels.get(revision) !== labels.get(equivalent)
    );
  });
}
