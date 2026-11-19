import Canvas from '~/components/Canvas';
import ProjectView from '~/components/ProjectView';
import Tooltip from '~/components/commons/Tooltip';
import Titlebar from '~/components/commons/Titlebar';
import StatusBar from '~/components/commons/StatusBar';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { useBackgroundNotify } from '~/components/commons/Notifications/backgroundWatch';

const App = () => {
  const { tabKey, activeId, activeTabId, activeTabMode } = useWorkspace();

  useBackgroundNotify(activeId, tabKey);

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
