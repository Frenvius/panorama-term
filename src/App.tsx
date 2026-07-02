import Canvas from '~/components/Canvas';
import Titlebar from '~/components/commons/Titlebar';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';

const App = () => {
  const { tabKey, activeTabId } = useWorkspace();

  return (
    <>
      <Titlebar />
      {activeTabId && <Canvas key={tabKey} />}
    </>
  );
};

export default App;
