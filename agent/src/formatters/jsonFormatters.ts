export function formatCompactJson(value: unknown): string {
  return JSON.stringify(value) ?? "null";
}
