import { routeCheckout } from "@/lib/checkout";

export function checkoutSummary(state: string): string {
  return `score:${routeCheckout(state)}`;
}
