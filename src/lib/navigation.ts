import type { ViewName } from "$lib/types";

const views = new Set<ViewName>(["overview", "setup", "settings", "logs"]);

export function isViewName(value: unknown): value is ViewName {
  return typeof value === "string" && views.has(value as ViewName);
}
