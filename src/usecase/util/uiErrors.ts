import { writeUiErrorLog } from '~/adapter/crashlog/crashlog.client';

interface ErrorDetails {
  componentStack?: string;
}

const describe = (error: unknown): string => {
  if (error instanceof Error) return error.stack ?? `${error.name}: ${error.message}`;
  return String(error);
};

export const reportUiError = (kind: string) => (error: unknown, details?: ErrorDetails) => {
  console.error(`[ui] ${kind}`, error, details?.componentStack ?? '');
  const entry = [`[${new Date().toISOString()}] ${kind} ${location.href}`, describe(error), details?.componentStack?.trim() ?? '']
    .filter(Boolean)
    .join('\n');
  writeUiErrorLog(entry).catch((e) => console.error('[ui] failed to write error log', e));
};
