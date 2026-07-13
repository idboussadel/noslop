import { makeId, formatLabel } from "@acme/shared";

export function buildApiPath(resource: string): string {
  return `/api/${resource}?id=${makeId()}`;
}

export const apiLabel = formatLabel("web");
