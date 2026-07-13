import { checkoutSummary } from "@/lib/cart";

/** Only referenced from tests — only-used-in-tests. */
export function testOnlyHelper(): string {
  return checkoutSummary("cart");
}
