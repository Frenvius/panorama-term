import { invoke } from '~/adapter/transport';

import type { DockerAction, DockerContainer } from '~/domain/interfaces/docker.interface';

export const dockerAvailable = (): Promise<boolean> => invoke<boolean>('docker_available');

export const dockerPs = (): Promise<DockerContainer[]> => invoke<DockerContainer[]>('docker_ps');

export const dockerAction = (id: string, action: DockerAction): Promise<void> =>
  invoke<void>('docker_action', { id, action });

export const dockerEngine = (action: 'start' | 'stop'): Promise<void> =>
  invoke<void>('docker_engine', { action });
