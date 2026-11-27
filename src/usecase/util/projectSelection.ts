import type { PanelGroup, ProjectViewState, ProjectPanelState } from '~/domain/interfaces/projectView.interface';

const keepGroupSelection = (incoming: PanelGroup, local: PanelGroup | undefined): PanelGroup => {
  const active = local?.activeTileId;
  if (!active || active === incoming.activeTileId || !incoming.tileIds.includes(active)) return incoming;
  return { ...incoming, activeTileId: active };
};

const keepPanelSelection = (incoming: ProjectPanelState, local: ProjectPanelState | undefined): ProjectPanelState => {
  if (!local) return incoming;
  const groups = Object.fromEntries(
    Object.entries(incoming.groups).map(([id, group]) => [id, keepGroupSelection(group, local.groups[id])])
  );
  const activeGroupId = incoming.groups[local.activeGroupId] ? local.activeGroupId : incoming.activeGroupId;
  return { ...incoming, groups, activeGroupId };
};

export const keepLocalSelection = (incoming: ProjectViewState, local: ProjectViewState): ProjectViewState => {
  const byProject = Object.fromEntries(
    Object.entries(incoming.byProject).map(([path, panel]) => [path, keepPanelSelection(panel, local.byProject[path])])
  );
  const keepProject = local.activeProject !== null && incoming.projects.includes(local.activeProject);
  return { ...incoming, byProject, activeProject: keepProject ? local.activeProject : incoming.activeProject };
};
