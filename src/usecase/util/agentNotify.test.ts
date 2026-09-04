import { expect, test } from 'bun:test';

import { createAgentNotifier } from '~/usecase/util/agentNotify';

const setup = (suppressed = false) => {
  const sent: string[] = [];
  const notifier = createAgentNotifier({
    focused: () => false,
    suppressed: () => suppressed,
    notify: (kind, text) => sent.push([kind, text].filter(Boolean).join(':')),
    clear: () => sent.push('clear')
  });
  return { sent, notifier };
};

test('busy to idle notifies, other transitions do not', () => {
  const { sent, notifier } = setup();
  notifier.onStatus('idle', 'busy');
  notifier.onStatus('busy', 'idle');
  notifier.onStatus('idle', 'waiting');
  expect(sent).toEqual(['finished', 'attention']);
});

test('suppressed notifier stays silent', () => {
  const { sent, notifier } = setup(true);
  notifier.onStatus('busy', 'idle');
  notifier.onNotify('title', 'body');
  notifier.onAgentEvent({ event: 'stop' });
  expect(sent).toEqual([]);
});

test('agent events take over status transitions', () => {
  const { sent, notifier } = setup();
  notifier.onAgentEvent({ event: 'stop', response: 'done' });
  notifier.onStatus('busy', 'idle');
  expect(sent).toEqual(['finished:done']);
});

test('pi stop identifies pi in the notification', () => {
  const { sent, notifier } = setup();
  notifier.onAgentEvent({ agent: 'pi', event: 'stop' });
  expect(sent).toEqual(['finished:Pi finished']);
});

test('prompt-submit clears and generic notify is muted right after an agent event', () => {
  const { sent, notifier } = setup();
  notifier.onAgentEvent({ event: 'prompt-submit' });
  notifier.onNotify('title', 'body');
  expect(sent).toEqual(['clear']);
});

test('idle event is muted inside the grace window of a previous notification', () => {
  const { sent, notifier } = setup();
  notifier.onAgentEvent({ event: 'permission', toolName: 'Bash', message: 'ls' });
  notifier.onAgentEvent({ event: 'notification', message: 'waiting' });
  expect(sent).toEqual(['permission:Bash: ls']);
});
