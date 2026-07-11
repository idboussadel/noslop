export function alpha(user: { name: string; age: number }) {
  const label = user.name.trim().toUpperCase();
  const score = user.age * 2 + 10;
  if (score > 50) {
    return { label, score, tier: "gold" };
  }
  return { label, score, tier: "silver" };
}
