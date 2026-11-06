import { invoke } from '@tauri-apps/api/core';

export const writeUiErrorLog = (entry: string): Promise<void> => invoke<void>('log_ui_error', { entry });
