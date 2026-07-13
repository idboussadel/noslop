export function makeId(): string {
  return Math.random().toString(36).slice(2, 10);
}

/** Never imported anywhere — unused export. */
export function deadId(): string {
  return "dead";
}
