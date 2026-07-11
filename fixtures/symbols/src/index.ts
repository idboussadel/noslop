import { Widget } from "./widget";
import { Color } from "./colors";
import { UsedType } from "./types";

const w = new Widget();
const v: UsedType = { a: 1 };
console.log(w.render(), Color.Red, v);

// `usedArg` is referenced; `deadArg` is a trailing unused parameter.
export function main(usedArg: number, deadArg: string): number {
  return usedArg;
}

main(1, "x");
