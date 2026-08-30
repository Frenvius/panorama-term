import Canvas from '~/components/Canvas';
import Tooltip from '~/components/commons/Tooltip';
import Titlebar from '~/components/commons/Titlebar';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { useBackgroundNotify } from '~/components/commons/Notifications/backgroundWatch';

const App = () => {
  const { tabKey, activeId, activeTabId } = useWorkspace();

  useBackgroundNotify(activeId, tabKey);

  return (
    <>
      <Titlebar />
      {activeTabId && <Canvas key={tabKey} />}
      <Tooltip />
    </>
  );
};

export default App;
