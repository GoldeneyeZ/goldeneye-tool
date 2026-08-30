export type WriteFn = (text: string) => void;

export function writeLine(write: WriteFn, text: string): void {
  write(`${text}\n`);
}
