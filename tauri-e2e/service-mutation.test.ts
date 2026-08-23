import assert from 'node:assert/strict';
import test from 'node:test';
import {
  executeServiceMutation,
  type ServiceMutationCommands,
  type ServiceType,
} from '../frontend/interface/src/ipc/service-mutation.js';

const serviceTypes: ServiceType[] = [
  'install',
  'uninstall',
  'start',
  'stop',
  'restart',
];

function successfulCommands(calls: string[]): ServiceMutationCommands {
  const success = async (name: string) => {
    calls.push(name);
    return { status: 'ok' as const, data: null };
  };
  return {
    installService: () => success('install'),
    uninstallService: () => success('uninstall'),
    startService: () => success('start'),
    stopService: () => success('stop'),
    restartService: () => success('restart'),
  };
}

test('service mutations dispatch to the matching generated command', async () => {
  for (const type of serviceTypes) {
    const calls: string[] = [];

    await executeServiceMutation(successfulCommands(calls), type);

    assert.deepEqual(calls, [type]);
  }
});

test('service mutations propagate generated command errors', async () => {
  for (const type of serviceTypes) {
    const calls: string[] = [];
    const commands = successfulCommands(calls);
    const commandName = `${type}Service` as keyof ServiceMutationCommands;
    commands[commandName] = async () => {
      calls.push(type);
      return { status: 'error' as const, error: `${type} blocked` };
    };

    await assert.rejects(
      () => executeServiceMutation(commands, type),
      (error) => error === `${type} blocked`,
    );
    assert.deepEqual(calls, [type]);
  }
});

test('service mutations reject unknown runtime input without invoking commands', async () => {
  const calls: string[] = [];

  await assert.rejects(
    () =>
      executeServiceMutation(
        successfulCommands(calls),
        'upgrade' as ServiceType,
      ),
    /Unsupported service mutation: upgrade/,
  );
  assert.deepEqual(calls, []);
});
