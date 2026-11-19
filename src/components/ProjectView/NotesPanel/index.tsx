import type { Tile } from '~/domain/interfaces/workspace.interface';

import React from 'react';
import { X, Plus } from 'lucide-react';

import { tileLabel } from '~/usecase/util/title';
import { notePreview } from '~/usecase/util/noteMeta';
import styles from '~/components/ProjectView/panels.module.scss';

export interface NotesPanelProps {
  notes: Tile[];
  activeTileId: string | null;
  onNewNote: () => void;
  onFocusNote: (tileId: string) => void;
  onCloseNote: (tileId: string) => void;
}

const NotesPanel = ({ notes, activeTileId, onNewNote, onFocusNote, onCloseNote }: NotesPanelProps) => (
  <>
    <div className={styles.notesHeader}>
      <span className={styles.notesTitle}>Project Notes ({notes.length})</span>
      <button type="button" className={styles.newNoteBtn} onClick={onNewNote}>
        <Plus size={12} strokeWidth={2} />
        New note
      </button>
    </div>
    <div className={styles.body}>
      {notes.length === 0 ? (
        <div className={styles.empty}>No notes for this project</div>
      ) : (
        <div className={styles.notesList}>
          {notes.map((note) => {
            const preview = notePreview(note.content);
            const isActive = activeTileId === note.id;
            const focusNote = () => onFocusNote(note.id);
            const closeNote = (e: React.MouseEvent) => {
              e.stopPropagation();
              onCloseNote(note.id);
            };
            return (
              <div key={note.id} onClick={focusNote} className={styles.noteCard} data-active={isActive || undefined}>
                <div className={styles.noteTop}>
                  <span className={styles.noteTitle}>{tileLabel(note)}</span>
                  <button type="button" onClick={closeNote} aria-label="Close note" className={styles.deleteNoteBtn}>
                    <X size={12} strokeWidth={2} />
                  </button>
                </div>
                {preview && <span className={styles.noteSnippet}>{preview}</span>}
              </div>
            );
          })}
        </div>
      )}
    </div>
  </>
);

export default NotesPanel;
