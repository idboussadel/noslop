import { formatPrice } from "@/lib/format";
import { debounce } from "lodash";

// `unusedName` is imported but never used → unused-import.
import { unusedName } from "@/lib/format";

export default function Page() {
  const handler = debounce(() => {}, 100);
  return formatPrice(42);
}
