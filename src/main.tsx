import { createRoot } from 'react-dom/client';

import App from '~/App';
import { WorkspaceProvider } from '~/usecase/context/WorkspaceContext';

import '~/styles/global.scss';

createRoot(document.getElementById('root')!).render(
  <WorkspaceProvider>
    <App />
  </WorkspaceProvider>
);
