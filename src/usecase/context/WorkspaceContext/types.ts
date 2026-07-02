import React from 'react';

import { useWorkspaces } from '~/usecase/hooks/useWorkspaces';

export type WorkspaceContextValue = ReturnType<typeof useWorkspaces>;

export interface WorkspaceProviderProps {
  children: React.ReactNode;
}
