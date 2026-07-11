// Used by page.tsx.
export function formatPrice(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

// Imported by name but never referenced in the importing file (unused-import there).
export const unusedName = "x";

// Exported, never imported anywhere → unused-export.
export function formatDead(x: number): number {
  return x * 2;
}

// Exported and used only in this file → API-surface finding, not deletion proof.
export function formatLocalOnly(x: number): number {
  return x + 1;
}

const localOnlyValue = formatLocalOnly(1);
