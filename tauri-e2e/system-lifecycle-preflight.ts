export interface SystemLifecyclePreflight {
  platform: NodeJS.Platform;
  optedIn: boolean;
  elevated: boolean;
  serviceStatus: string;
}

export function assertSystemLifecyclePreflight(
  input: SystemLifecyclePreflight,
): void {
  if (!input.optedIn) {
    throw new Error(
      'System lifecycle E2E requires CHIMERA_E2E_SYSTEM_LIFECYCLE=1.',
    );
  }
  if (input.platform !== 'win32') {
    throw new Error('System lifecycle E2E is only supported on Windows.');
  }
  if (!input.elevated) {
    throw new Error(
      'System lifecycle E2E requires an elevated Windows runner/VM.',
    );
  }
  if (input.serviceStatus !== 'not_installed') {
    throw new Error(
      'System lifecycle E2E refuses to take ownership of an existing Chimera Service: ' +
        input.serviceStatus,
    );
  }
}
