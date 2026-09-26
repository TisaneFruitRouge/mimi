/**
 * How each tool's arguments read on an approval card. Integrations add an entry for
 * their tools; anything without one falls back to a tidy list of its arguments.
 */
export type ArgRow = { label: string; value: string };

type Formatter = (args: Record<string, unknown>) => ArgRow[];

export const formatters: Record<string, Formatter> = {};

export function describeArgs(tool: string, args: Record<string, unknown>): ArgRow[] {
  const format = formatters[tool];
  if (format) return format(args);
  return Object.entries(args ?? {})
    .filter(([, v]) => v !== null && v !== undefined && v !== "")
    .map(([k, v]) => ({
      label: humanize(k),
      value: typeof v === "string" ? v : JSON.stringify(v, null, 1),
    }));
}

function humanize(key: string) {
  const words = key.replace(/[_-]+/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
