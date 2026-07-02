import React from 'react';

import { useWorkspaces } from '~/usecase/hooks/useWorkspaces';
import { WorkspaceContextValue, WorkspaceProviderProps } from './types';

const WorkspaceContext = React.createContext({} as WorkspaceContextValue);

export const WorkspaceProvider = ({ children }: WorkspaceProviderProps) => {
  const value = useWorkspaces();
  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
};

export const useWorkspace = () => React.useContext(WorkspaceContext);
