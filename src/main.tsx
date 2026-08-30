import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';

import App from '~/App';
import { initTheme } from '~/usecase/util/theme';
import { initPrefs } from '~/usecase/util/prefs';
import { hydrateAlerts } from '~/usecase/util/alerts';
import { loadHackFont } from '~/usecase/util/fontUtils';
import { initSettings } from '~/adapter/settings/settings.client';
import NotificationOverlay from '~/components/commons/Notifications';
import { WorkspaceProvider } from '~/usecase/context/WorkspaceContext';

import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/400-italic.css';
import '@fontsource/jetbrains-mono/700.css';
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
const boot = async (): Promise<void> => {
  await initSettings().catch(() => {});
  if (!isOverlay) {
    await initPrefs().catch(() => {});
    hydrateAlerts();
  }
  initTheme();
  if (isOverlay) mountOverlay();
  else mountApp();
};

void boot();
