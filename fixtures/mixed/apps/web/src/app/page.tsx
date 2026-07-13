import { formatPrice } from "@/lib/format";
import { buildApiPath } from "@/lib/api";
import { Button } from "@/components/Button";
import { debounce } from "lodash";

// `unusedName` is imported but never used → unused-import.
import { unusedName } from "@/lib/format";

export default function Page() {
  const handler = debounce(() => {}, 100);
  void handler;
  return (
    <>
      <Button label={formatPrice(4200)} />
      <span>{buildApiPath("users")}</span>
    </>
  );
}
