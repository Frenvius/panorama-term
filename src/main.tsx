import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';

import App from '~/App';
import { initTheme } from '~/usecase/util/theme';
import { loadHackFont } from '~/usecase/util/fontUtils';
import { initSettings } from '~/adapter/settings/settings.client';
import NotificationOverlay from '~/components/commons/Notifications';
import { WorkspaceProvider } from '~/usecase/context/WorkspaceContext';

import '~/styles/global.scss';

const root = () => createRoot(document.getElementById('root')!);

const mountApp = () =>
  root().render(
    <WorkspaceProvider>
      <App />
    </WorkspaceProvider>
  );

const mountOverlay = () => {
  document.documentElement.style.background = 'transparent';
  document.body.style.background = 'transparent';
  root().render(<NotificationOverlay />);
};

const isOverlay = getCurrentWindow().label === 'notif';

void loadHackFont();
void initSettings().finally(() => {
  initTheme();
  if (isOverlay) mountOverlay();
  else mountApp();
});
