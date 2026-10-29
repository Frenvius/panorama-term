export interface ShellSummary {
  cmd: string;
  out: string;
}

const PROMPTS = [/^PS [^>]*>\s?(.*)$/, /^[\w.-]+@[\w.-]+(?::[^$#]*)?[$#]\s?(.*)$/, /^[❯›➜λ$#](?:\s+(.*))?$/];

const promptCommand = (line: string): string | null => {
  for (const re of PROMPTS) {
    const m = line.match(re);
    if (m) return (m[1] ?? '').trim();
  }
  return null;
};

export const shellSummary = (lines: string[]): ShellSummary | null => {
  const rows = lines.map((l) => l.trimEnd());
  let end = rows.length;
  for (let i = rows.length - 1; i >= 0; i--) {
    const cmd = promptCommand(rows[i]);
    if (cmd === null) continue;
    if (cmd) {
      const header = rows[i - 1]?.trim();
      const output = rows.slice(i + 1, end).filter((l) => l.trim());
      if (header && output[output.length - 1]?.trim() === header) output.pop();
      return { cmd, out: output.pop()?.trim() ?? '' };
    }
    end = i;
  }
  return null;
};
