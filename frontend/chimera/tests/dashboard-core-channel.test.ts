import assert from 'node:assert/strict';
import test from 'node:test';
import { coreControllerChannel } from '../src/components/widgets/dashboard-core-channel.ts';

test('shows the ref control channel labels for each actual CoreActor endpoint', () => {
  assert.equal(coreControllerChannel({ Http: '127.0.0.1:9090' }), 'HTTP');
  assert.equal(
    coreControllerChannel({ NamedPipe: 'chimera-core' }),
    'IPC · Named Pipe',
  );
  assert.equal(
    coreControllerChannel({ UnixSocket: '/run/user/1000/core.sock' }),
    'IPC · Unix Socket',
  );
});

test('does not invent a channel when CoreActor status is missing', () => {
  assert.equal(coreControllerChannel(null), '—');
  assert.equal(coreControllerChannel(undefined), '—');
});
