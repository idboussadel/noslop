import { usedLib, staleFlag } from "./lib";

console.log(usedLib, staleFlag);

// An @internal export in an *entry* file re-enables unused analysis, so this
// unreferenced helper must be flagged despite living in an entry.
/** @internal -- not part of the public API */
export function internalHelper(): number {
  return 1;
}
