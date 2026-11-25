import React from 'react';
import Canvas from '~/components/Canvas';
import ProjectView from '~/components/ProjectView';
import Tooltip from '~/components/commons/Tooltip';
import Titlebar from '~/components/commons/Titlebar';
import StatusBar from '~/components/commons/StatusBar';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { useBackgroundNotify } from '~/components/commons/Notifications/backgroundWatch';
import { useRemoteUpdate } from '~/usecase/hooks/useUpdater';

const App = () => {
  const { tabKey, activeTabId, activeTabMode } = useWorkspace();

  useBackgroundNotify();
  useRemoteUpdate();

  React.useEffect(() => {
    const blockBrowserFind = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f' && !e.defaultPrevented) {
        e.preventDefault();
      }
    };
    window.addEventListener('keydown', blockBrowserFind);
    return () => window.removeEventListener('keydown', blockBrowserFind);
  }, []);

  return (
    <>
      <Titlebar />
      {activeTabId && (activeTabMode === 'project' ? <ProjectView key={tabKey} /> : <Canvas key={tabKey} />)}
      <StatusBar />
      <Tooltip />
    </>
  );
};

export default App;
