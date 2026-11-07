export const formatElapsed = (ms: number): string => {
  const total = Math.max(0, Math.floor(ms / 1000));
  const min = Math.floor(total / 60);
  return min ? `${min}m ${total % 60}s` : `${total}s`;
};
