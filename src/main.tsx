import { createRoot } from 'react-dom/client';

import App from '~/App';
import { initTheme } from '~/usecase/util/theme';
import { loadHackFont } from '~/usecase/util/fontUtils';
import { initSettings } from '~/adapter/settings/settings.client';
import { WorkspaceProvider } from '~/usecase/context/WorkspaceContext';

import '~/styles/global.scss';

const mount = () =>
  createRoot(document.getElementById('root')!).render(
    <WorkspaceProvider>
      <App />
    </WorkspaceProvider>
  );

void loadHackFont();
void initSettings().finally(() => {
  initTheme();
  mount();
});
