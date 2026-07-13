/** Messy handler for demo complexity findings. */
export function routeCheckout(state: string): number {
  let score = 0;
  if (state === "cart") score += 1;
  if (state === "shipping") score += 2;
  if (state === "payment") score += 3;
  if (state === "review") score += 4;
  if (state === "done") score += 5;
  if (state === "cancel") score -= 1;
  if (state === "error") score -= 2;
  if (state === "retry") score += 1;
  return score;
}
