export interface OrchNotification {
  runId: string;
  runTitle: string;
  kind: 'gate' | 'review' | 'exhausted' | 'conflict' | 'completed';
  taskId?: string | null;
  taskTitle?: string | null;
  detail?: string | null;
}

export const orchNotificationText = (event: OrchNotification): string => {
  switch (event.kind) {
    case 'gate': return `Gate waiting: ${event.detail || 'Decision needed'}`;
    case 'review': return `Task ready for review: ${event.taskTitle || 'Task'}`;
    case 'exhausted': return `Task failed after all attempts: ${event.taskTitle || 'Task'}`;
    case 'conflict': return `Merge conflict in ${event.taskTitle || 'task'}${event.detail ? `: ${event.detail}` : ''}`;
    case 'completed': return 'Board finished';
  }
};
