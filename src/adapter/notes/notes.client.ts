import { invoke } from '@tauri-apps/api/core';

export const readNote = (wsId: string, noteId: string): Promise<string | null> =>
  invoke('read_note', { wsId, noteId });

export const writeNote = (wsId: string, noteId: string, content: string): Promise<void> =>
  invoke('write_note', { wsId, noteId, content });

export const deleteNote = (wsId: string, noteId: string): Promise<void> =>
  invoke('delete_note', { wsId, noteId });

export const linkNote = (wsId: string, noteId: string, termTileId: string, title: string): Promise<string> =>
  invoke('link_note', { wsId, noteId, termTileId, title });

export const unlinkNote = (noteId: string, termTileId: string): Promise<void> =>
  invoke('unlink_note', { noteId, termTileId });

export const linkTerm = (aId: string, aName: string, bId: string, bName: string): Promise<void> =>
  invoke('link_term', { aId, aName, bId, bName });

export const unlinkTerm = (aId: string, bId: string): Promise<void> =>
  invoke('unlink_term', { aId, bId });
