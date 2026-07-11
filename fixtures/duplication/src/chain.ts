// Long chains of calls to a shared abstraction are NOT duplication worth fixing.
import { pipe, mapIt, filterIt, reduceIt, tapIt, logIt, doneIt } from "./helpers";

export const r1 = pipe(mapIt(filterIt(reduceIt(tapIt(logIt(doneIt(1)))))));
export const r2 = pipe(mapIt(filterIt(reduceIt(tapIt(logIt(doneIt(2)))))));
