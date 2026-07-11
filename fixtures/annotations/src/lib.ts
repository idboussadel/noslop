export const usedLib = 1;

/** @public -- consumed by plugin SDKs */
export function publicApi(): number {
  return 1;
}

export function trulyDead(): number {
  return 2;
}

/** @expected-unused -- API lands in v2 */
export const futureFlag = false;

/** @expected-unused */
export const noReasonFlag = false;

/** @expected-unused -- should have been removed */
export const staleFlag = true;
