const lastFocus = new Map<string, number>();

export const touchTile = (id: string): void => {
  lastFocus.set(id, Date.now());
};

export const byRecency = <T extends { id: string }>(items: T[]): T[] =>
  [...items].sort((a, b) => (lastFocus.get(b.id) ?? 0) - (lastFocus.get(a.id) ?? 0));
