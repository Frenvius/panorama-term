import { storeList, storeRead, storeWrite, storeDelete } from '~/adapter/store/store.client';
import {
  TabMeta,
  TabState,
  CanvasState,
  WorkspaceFile,
  WorkspaceMeta,
  WorkspaceIndex
} from '~/domain/interfaces/workspace.interface';

const INDEX_FILE = 'workspaces.json';
const WS_DIR = 'workspaces';

const COLORS = ['#6ea8fe', '#75d0a0', '#e6a957', '#d98abf', '#7fd1d6', '#d97c7c', '#b39ddb', '#c0c97f'];

const uuid = (): string => crypto.randomUUID();

const newId = (): string => `ws-${uuid()}`;

const newTabId = (): string => `tab-${uuid()}`;

const sanitizeCoord = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0);

const emptyState = (): CanvasState => ({ version: 1, tiles: [], viewport: { zoom: 1, centerX: 0, centerY: 0 } });

const emptyTab = (name: string): TabState => ({ id: newTabId(), name, state: emptyState() });

const wsFile = (id: string): string => `${WS_DIR}/${id}.json`;

const nextTabName = (tabs: TabState[]): string => {
  let max = 0;
  for (const t of tabs) {
    const m = /^Tab (\d+)$/.exec(t.name);
    if (m) max = Math.max(max, Number(m[1]));
  }
  return `Tab ${max + 1}`;
};

const normalizeState = (state: CanvasState | null): CanvasState => {
  if (!state || state.version !== 1) return emptyState();
  for (const tile of state.tiles ?? []) {
    tile.x = sanitizeCoord(tile.x);
    tile.y = sanitizeCoord(tile.y);
  }
  if (!state.viewport) state.viewport = emptyState().viewport;
  return state;
};

const migrate = (raw: unknown): WorkspaceFile | null => {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  if (!r.meta) return null;
  const meta = r.meta as WorkspaceMeta;

  let tabs: TabState[];
  if (Array.isArray(r.tabs) && r.tabs.length > 0) {
    tabs = (r.tabs as TabState[]).map((t) => ({
      id: t.id ?? newTabId(),
      name: t.name ?? 'Tab',
      state: normalizeState(t.state ?? null)
    }));
  } else {
    tabs = [{ id: newTabId(), name: 'Tab 1', state: normalizeState((r.state as CanvasState) ?? null) }];
  }

  let activeTabId = r.activeTabId as string | undefined;
  if (!activeTabId || !tabs.some((t) => t.id === activeTabId)) activeTabId = tabs[0]!.id;
  return { meta, tabs, activeTabId };
};

class Service {
  private cachedIndex: WorkspaceIndex | null = null;
  private loadedTabs = new Set<string>();

  private tabOf(file: WorkspaceFile, tabId: string): TabState | undefined {
    return file.tabs.find((t) => t.id === tabId);
  }

  private async writeWorkspace(file: WorkspaceFile): Promise<void> {
    await storeWrite(wsFile(file.meta.id), file);
  }

  private async readWorkspace(id: string): Promise<WorkspaceFile | null> {
    const raw = await storeRead<unknown>(wsFile(id));
    const file = migrate(raw);
    if (!file) return null;
    if (raw && typeof raw === 'object' && !('tabs' in raw)) await this.writeWorkspace(file);
    return file;
  }

  private async loadIndex(): Promise<WorkspaceIndex> {
    if (this.cachedIndex) return this.cachedIndex;
    this.cachedIndex = await this.initIndex();
    return this.cachedIndex;
  }

  private async saveIndex(idx: WorkspaceIndex): Promise<void> {
    this.cachedIndex = idx;
    await storeWrite(INDEX_FILE, idx);
  }

  private async initIndex(): Promise<WorkspaceIndex> {
    const existing = await storeRead<WorkspaceIndex>(INDEX_FILE);
    if (existing && existing.order.length > 0) {
      if (!existing.activeId || !existing.order.includes(existing.activeId)) {
        existing.activeId = existing.order[0] ?? null;
      }
      return existing;
    }

    const orphaned = (await storeList(WS_DIR)).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5));
    if (orphaned.length > 0) {
      const idx: WorkspaceIndex = { version: 1, activeId: orphaned[0]!, order: orphaned };
      await storeWrite(INDEX_FILE, idx);
      return idx;
    }

    const id = newId();
    const now = Date.now();
    const meta: WorkspaceMeta = { id, name: 'Workspace 1', color: COLORS[0]!, createdAt: now, lastFocusedAt: now };
    const firstTab = emptyTab('Tab 1');
    await this.writeWorkspace({ meta, tabs: [firstTab], activeTabId: firstTab.id });
    const idx: WorkspaceIndex = { version: 1, activeId: id, order: [id] };
    await storeWrite(INDEX_FILE, idx);
    return idx;
  }

  private async activeRef(): Promise<{ workspaceId: string; tabId: string } | null> {
    const idx = await this.loadIndex();
    if (!idx.activeId) return null;
    const file = await this.readWorkspace(idx.activeId);
    if (!file) return null;
    return { workspaceId: idx.activeId, tabId: file.activeTabId };
  }

  async list(): Promise<{ activeId: string | null; workspaces: WorkspaceMeta[] }> {
    const idx = await this.loadIndex();
    const workspaces: WorkspaceMeta[] = [];
    for (const id of idx.order) {
      const file = await this.readWorkspace(id);
      if (file) workspaces.push(file.meta);
    }
    return { activeId: idx.activeId, workspaces };
  }

  async setActive(id: string): Promise<void> {
    const idx = await this.loadIndex();
    if (!idx.order.includes(id)) return;
    idx.activeId = id;
    await this.saveIndex(idx);
    const file = await this.readWorkspace(id);
    if (file) {
      file.meta.lastFocusedAt = Date.now();
      await this.writeWorkspace(file);
    }
  }

  async create(name?: string): Promise<WorkspaceMeta> {
    const idx = await this.loadIndex();
    const id = newId();
    const now = Date.now();
    const meta: WorkspaceMeta = {
      id,
      name: name?.trim() || `Workspace ${idx.order.length + 1}`,
      color: COLORS[idx.order.length % COLORS.length]!,
      createdAt: now,
      lastFocusedAt: now
    };
    const firstTab = emptyTab('Tab 1');
    await this.writeWorkspace({ meta, tabs: [firstTab], activeTabId: firstTab.id });
    idx.order.push(id);
    await this.saveIndex(idx);
    return meta;
  }

  async rename(id: string, name: string): Promise<void> {
    const file = await this.readWorkspace(id);
    const trimmed = name.trim();
    if (!file || !trimmed) return;
    file.meta.name = trimmed;
    await this.writeWorkspace(file);
  }

  async setColor(id: string, color: string): Promise<void> {
    const file = await this.readWorkspace(id);
    if (!file) return;
    file.meta.color = color;
    await this.writeWorkspace(file);
  }

  async delete(id: string): Promise<{ activeId: string | null; deleted: boolean }> {
    const idx = await this.loadIndex();
    if (idx.order.length <= 1 || !idx.order.includes(id)) return { activeId: idx.activeId, deleted: false };
    const removedAt = idx.order.indexOf(id);
    idx.order.splice(removedAt, 1);
    if (idx.activeId === id) idx.activeId = idx.order[Math.max(0, removedAt - 1)] ?? idx.order[0]!;
    await this.saveIndex(idx);
    await storeDelete(wsFile(id));
    return { activeId: idx.activeId, deleted: true };
  }

  async reorder(order: string[]): Promise<void> {
    const idx = await this.loadIndex();
    const known = new Set(idx.order);
    const next = order.filter((wid) => known.has(wid));
    for (const wid of idx.order) if (!next.includes(wid)) next.push(wid);
    idx.order = next;
    await this.saveIndex(idx);
  }

  async getTabs(workspaceId: string): Promise<{ activeTabId: string | null; tabs: TabMeta[] }> {
    const file = await this.readWorkspace(workspaceId);
    if (!file) return { activeTabId: null, tabs: [] };
    return { activeTabId: file.activeTabId, tabs: file.tabs.map((t) => ({ id: t.id, name: t.name })) };
  }

  async loadTabState(workspaceId: string, tabId: string): Promise<CanvasState | null> {
    const file = await this.readWorkspace(workspaceId);
    const tab = file && this.tabOf(file, tabId);
    if (tab) {
      this.loadedTabs.add(`${workspaceId}/${tabId}`);
      return normalizeState(tab.state);
    }
    return null;
  }

  async saveTabState(workspaceId: string, tabId: string, state: CanvasState): Promise<void> {
    const file = await this.readWorkspace(workspaceId);
    if (!file) return;
    const tab = this.tabOf(file, tabId);
    if (!tab) return;
    const normalized = normalizeState(state);
    const key = `${workspaceId}/${tabId}`;
    if (normalized.tiles.length === 0 && tab.state.tiles.length > 0 && !this.loadedTabs.has(key)) return;
    tab.state = normalized;
    await this.writeWorkspace(file);
  }

  async setActiveTab(workspaceId: string, tabId: string): Promise<void> {
    const file = await this.readWorkspace(workspaceId);
    if (!file || !this.tabOf(file, tabId)) return;
    file.activeTabId = tabId;
    await this.writeWorkspace(file);
  }

  async createTab(workspaceId: string, name?: string): Promise<TabMeta | null> {
    const file = await this.readWorkspace(workspaceId);
    if (!file) return null;
    const tab = emptyTab(name?.trim() || nextTabName(file.tabs));
    file.tabs.push(tab);
    await this.writeWorkspace(file);
    return { id: tab.id, name: tab.name };
  }

  async renameTab(workspaceId: string, tabId: string, name: string): Promise<void> {
    const file = await this.readWorkspace(workspaceId);
    if (!file) return;
    const tab = this.tabOf(file, tabId);
    const trimmed = name.trim();
    if (!tab || !trimmed) return;
    tab.name = trimmed;
    await this.writeWorkspace(file);
  }

  async deleteTab(workspaceId: string, tabId: string): Promise<{ activeTabId: string | null; deleted: boolean }> {
    const file = await this.readWorkspace(workspaceId);
    if (!file || file.tabs.length <= 1) return { activeTabId: file?.activeTabId ?? null, deleted: false };
    const removedAt = file.tabs.findIndex((t) => t.id === tabId);
    if (removedAt === -1) return { activeTabId: file.activeTabId, deleted: false };
    file.tabs.splice(removedAt, 1);
    if (file.activeTabId === tabId) file.activeTabId = file.tabs[Math.max(0, removedAt - 1)]!.id;
    await this.writeWorkspace(file);
    return { activeTabId: file.activeTabId, deleted: true };
  }

  async loadActiveState(): Promise<CanvasState | null> {
    const ref = await this.activeRef();
    return ref ? this.loadTabState(ref.workspaceId, ref.tabId) : null;
  }

  async saveActiveState(state: CanvasState): Promise<void> {
    const ref = await this.activeRef();
    if (ref) await this.saveTabState(ref.workspaceId, ref.tabId, state);
  }
}

export const workspaceService = new Service();
