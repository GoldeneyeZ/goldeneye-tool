import type { SelectedSymbol } from "../domain/types.js";

export function contextAffordanceWarnings(selected: SelectedSymbol): string[] {
  const warnings: string[] = [];

  if ((selected.lines ?? 0) >= 80) {
    warnings.push("large method; source likely needed");
  }

  if ((selected.complexity ?? 0) >= 10 || (selected.cognitive ?? 0) >= 15) {
    warnings.push("high complexity; inspect related callers and tests before editing");
  }

  if ((selected.callers ?? 0) > 8) {
    warnings.push("high caller count; use callers command rather than inline trace");
  }

  return warnings;
}
