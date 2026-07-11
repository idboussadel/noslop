// Same body as alpha with renamed identifiers — caught only in semantic mode.
export function beta(person: { name: string; age: number }) {
  const lbl = person.name.trim().toUpperCase();
  const pts = person.age * 2 + 10;
  if (pts > 50) {
    return { lbl, pts, tier: "gold" };
  }
  return { lbl, pts, tier: "silver" };
}
