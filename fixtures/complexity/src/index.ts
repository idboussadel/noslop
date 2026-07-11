export function simple(a: number): number {
  return a + 1;
}

export function gnarly(x: number, y: number): number {
  if (x > 0) {
    for (let i = 0; i < x; i++) {
      if (y > i && x < 10) {
        while (y > 0) {
          if (i === y) {
            return 1;
          }
        }
      }
    }
  } else if (x < -5) {
    return 2;
  }
  return 0;
}
