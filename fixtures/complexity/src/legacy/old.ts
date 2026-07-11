// Deliberately complex, but exempted by the override — must NOT be flagged.
export function legacyFlow(x: number, y: number): number {
  if (x) {
    for (let i = 0; i < x; i++) {
      if (y > i && x < 10) {
        while (y > 0) {
          if (i === y) {
            return 1;
          }
        }
      }
    }
  }
  return 0;
}
