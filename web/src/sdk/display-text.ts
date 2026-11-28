// Display only: stored identifiers, control values and link destinations keep
// their full UUIDs. Generated resource prefixes and revision suffixes are not
// part of the eight-character label.
const uuid =
  /(?:[a-z][a-z0-9_-]{0,63}[.:-])?\b([0-9a-f]{8})-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b(?::[1-9]\d*)?/gi;

export function displayText<T>(value: T): T {
  if (typeof value === "string") return value.replace(uuid, "$1") as T;
  if (Array.isArray(value)) return value.map(displayText) as T;
  return value;
}

export function shortId(value: string | null | undefined): string {
  return value ? displayText(value).slice(0, 8) : "—";
}

export function truncateText(value: string, limit: number): string {
  // Keep a UUID whole until the display formatter can abbreviate it. This also
  // preserves the original Markdown source and link destinations.
  const boundary = [...value.matchAll(uuid)].find(
    (match) => match.index < limit && match.index + match[0].length > limit,
  );
  return value.slice(0, boundary ? boundary.index + boundary[0].length : limit);
}
