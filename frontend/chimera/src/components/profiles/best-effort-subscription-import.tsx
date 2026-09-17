import {
  commands,
  unwrapResult,
  useProfile,
  useSetting,
  type NetworkProbeResult,
} from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import {
  CheckCircleOutlined,
  CloudDownloadOutlined,
  ErrorOutlined,
  LanguageOutlined,
  NetworkCheckOutlined,
  RemoveCircleOutlined,
  SettingsEthernetOutlined,
  SpeedOutlined,
} from '@mui/icons-material';
import {
  Alert,
  Button,
  Card,
  CardContent,
  CardHeader,
  CircularProgress,
  Stack,
  Typography,
} from '@mui/material';
import { useLockFn } from 'ahooks';
import { useState, type ReactNode } from 'react';
import { QuickImport } from '@/components/profiles/quick-import';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';

type StepId =
  | 'default-import'
  | 'network-setup'
  | 'address-check'
  | 'stability-check'
  | 'direct-import';

type StepStatus = 'pending' | 'running' | 'success' | 'failure' | 'skipped';

type StepState = {
  status: StepStatus;
  detail?: string;
};

type ImportResult = {
  status: 'success' | 'failure';
  detail: string;
};

type ProxyDecision = 'idle' | 'enabled' | 'skipped' | 'failure';
type HostKind = 'domain' | 'ip';

const PROBE_TIMEOUT_MS = 5_000;
const STABILITY_ATTEMPTS = 3;
const STABILITY_LATENCY_SPREAD_MS = 3_000;

const getStepDefinitions = (): Array<{
  id: StepId;
  title: string;
  description: string;
  icon: ReactNode;
}> => [
  {
    id: 'default-import',
    title: m.subscription_onboarding_step_default_title(),
    description: m.subscription_onboarding_step_default_description(),
    icon: <CloudDownloadOutlined />,
  },
  {
    id: 'network-setup',
    title: m.subscription_onboarding_step_network_setup_title(),
    description: m.subscription_onboarding_step_network_setup_description(),
    icon: <SettingsEthernetOutlined />,
  },
  {
    id: 'address-check',
    title: m.subscription_onboarding_step_address_check_title(),
    description: m.subscription_onboarding_step_address_check_description(),
    icon: <LanguageOutlined />,
  },
  {
    id: 'stability-check',
    title: m.subscription_onboarding_step_stability_title(),
    description: m.subscription_onboarding_step_stability_description(),
    icon: <SpeedOutlined />,
  },
  {
    id: 'direct-import',
    title: m.subscription_onboarding_step_direct_title(),
    description: m.subscription_onboarding_step_direct_description(),
    icon: <NetworkCheckOutlined />,
  },
];

const createInitialSteps = (): Record<StepId, StepState> => ({
  'default-import': { status: 'pending' },
  'network-setup': { status: 'pending' },
  'address-check': { status: 'pending' },
  'stability-check': { status: 'pending' },
  'direct-import': { status: 'pending' },
});

const getHostKind = (url: string): HostKind => {
  const parsed = new URL(url);

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error(m.subscription_onboarding_invalid_protocol());
  }

  if (!parsed.hostname) {
    throw new Error(m.subscription_onboarding_missing_host());
  }

  return /^[0-9.]+$/.test(parsed.hostname) || parsed.hostname.includes(':')
    ? 'ip'
    : 'domain';
};

const probe = async (url: string): Promise<NetworkProbeResult> => {
  return unwrapResult(
    await commands.probeNetwork({
      url,
      expected_status: null,
      timeout_ms: PROBE_TIMEOUT_MS,
    }),
  );
};

const formatProbeError = (error: unknown) => {
  const value = String(error);

  if (value.includes('resolution_failed')) {
    return m.subscription_onboarding_probe_dns_failed();
  }

  if (value.includes('timed_out')) {
    return m.subscription_onboarding_probe_timed_out();
  }

  if (value.includes('target_blocked')) {
    return m.subscription_onboarding_probe_target_blocked();
  }

  return formatError(error);
};

function StepCard({
  index,
  title,
  description,
  icon,
  id,
  state,
}: {
  index: number;
  title: string;
  description: string;
  icon: ReactNode;
  id: StepId;
  state: StepState;
}) {
  const statusIcon =
    state.status === 'running' ? (
      <CircularProgress size={22} />
    ) : state.status === 'success' ? (
      <CheckCircleOutlined color="success" />
    ) : state.status === 'failure' ? (
      <ErrorOutlined color="error" />
    ) : state.status === 'skipped' ? (
      <RemoveCircleOutlined color="disabled" />
    ) : (
      <span className="flex size-6 items-center justify-center rounded-full bg-black/8 text-sm dark:bg-white/12">
        {index + 1}
      </span>
    );

  const statusLabel = {
    pending: m.subscription_onboarding_status_pending(),
    running: m.subscription_onboarding_status_running(),
    success: m.subscription_onboarding_status_success(),
    failure: m.subscription_onboarding_status_failure(),
    skipped: m.subscription_onboarding_status_skipped(),
  }[state.status];

  return (
    <Card
      variant="outlined"
      data-slot={`best-effort-step-${id}`}
      data-status={state.status}
    >
      <CardHeader
        avatar={icon}
        title={
          <div className="flex items-center justify-between gap-3">
            <Typography component="span" variant="subtitle1">
              {title}
            </Typography>
            <div className="flex shrink-0 items-center gap-2 text-sm">
              {statusIcon}
              <span>{statusLabel}</span>
            </div>
          </div>
        }
        subheader={description}
      />
      {state.detail && (
        <CardContent className="!pt-0">
          <Typography color="text.secondary" variant="body2">
            {state.detail}
          </Typography>
        </CardContent>
      )}
    </Card>
  );
}

export function BestEffortSubscriptionImport() {
  const stepDefinitions = getStepDefinitions();
  const { create } = useProfile();
  const systemProxy = useSetting('enable_system_proxy');
  const tunMode = useSetting('enable_tun_mode');
  const [url, setUrl] = useState('');
  const [steps, setSteps] = useState(createInitialSteps);
  const [isRunning, setIsRunning] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [proxyDecision, setProxyDecision] = useState<ProxyDecision>('idle');

  const updateStep = (id: StepId, state: StepState) => {
    setSteps((current) => ({ ...current, [id]: state }));
  };

  const skipSteps = (...ids: StepId[]) => {
    setSteps((current) => {
      const next = { ...current };

      for (const id of ids) {
        if (next[id].status === 'pending') {
          next[id] = {
            status: 'skipped',
            detail: m.subscription_onboarding_skip_detail(),
          };
        }
      }

      return next;
    });
  };

  const restoreNetworkAfterFailure = async (
    originalProxy: boolean,
    originalTun: boolean,
  ) => {
    const errors: string[] = [];

    if (originalTun) {
      try {
        await tunMode.upsert(true);
      } catch (error) {
        errors.push(
          m.subscription_onboarding_restore_tun_failed({
            error: formatError(error),
          }),
        );
      }
    }

    if (originalProxy) {
      try {
        await systemProxy.upsert(true);
      } catch (error) {
        errors.push(
          m.subscription_onboarding_restore_proxy_failed({
            error: formatError(error),
          }),
        );
      }
    }

    return errors;
  };

  const finishFailure = async (
    detail: string,
    originalProxy: boolean,
    originalTun: boolean,
  ) => {
    const restoreErrors = await restoreNetworkAfterFailure(
      originalProxy,
      originalTun,
    );
    const restoration = restoreErrors.length
      ? ` ${restoreErrors.join(' ')}`
      : ` ${m.subscription_onboarding_restore_success()}`;

    setResult({
      status: 'failure',
      detail: m.subscription_onboarding_failure_result({
        detail,
        restoration,
      }),
    });
  };

  const handleImport = useLockFn(async (inputUrl: string) => {
    const target = inputUrl.trim();

    if (!target || isRunning) {
      return;
    }

    const originalProxy = Boolean(systemProxy.value);
    const originalTun = Boolean(tunMode.value);

    setIsRunning(true);
    setResult(null);
    setProxyDecision('idle');
    setSteps(createInitialSteps());

    try {
      updateStep('default-import', { status: 'running' });

      try {
        await create.mutateAsync({
          type: 'url',
          data: { url: target, option: null, mode: 'default' },
        });
        updateStep('default-import', {
          status: 'success',
          detail: m.subscription_onboarding_default_import_success(),
        });
        skipSteps(
          'network-setup',
          'address-check',
          'stability-check',
          'direct-import',
        );
        setResult({
          status: 'success',
          detail: m.subscription_onboarding_import_success_default(),
        });
        return;
      } catch (error) {
        updateStep('default-import', {
          status: 'failure',
          detail: formatError(error),
        });
      }

      let hostKind: string;
      try {
        hostKind = getHostKind(target);
      } catch (error) {
        updateStep('address-check', {
          status: 'failure',
          detail: formatError(error),
        });
        skipSteps('stability-check', 'direct-import');
        await finishFailure(
          m.subscription_onboarding_failure_invalid_url(),
          originalProxy,
          originalTun,
        );
        return;
      }

      updateStep('network-setup', { status: 'running' });
      const setupErrors: string[] = [];

      if (originalProxy) {
        try {
          await systemProxy.upsert(false);
        } catch (error) {
          setupErrors.push(
            m.subscription_onboarding_disable_system_proxy_failed({
              error: formatError(error),
            }),
          );
        }
      }

      if (originalTun) {
        try {
          await tunMode.upsert(false);
        } catch (error) {
          setupErrors.push(
            m.subscription_onboarding_disable_tun_failed({
              error: formatError(error),
            }),
          );
        }
      }

      if (setupErrors.length) {
        updateStep('network-setup', {
          status: 'failure',
          detail: setupErrors.join(' '),
        });
        skipSteps('address-check', 'stability-check', 'direct-import');
        await finishFailure(
          m.subscription_onboarding_network_setup_failed(),
          originalProxy,
          originalTun,
        );
        return;
      }

      updateStep('network-setup', {
        status: 'success',
        detail: m.subscription_onboarding_network_setup_success(),
      });

      updateStep('address-check', { status: 'running' });

      try {
        const addressResult = await probe(target);
        updateStep('address-check', {
          status: 'success',
          detail: m.subscription_onboarding_address_success({
            host_kind:
              hostKind === 'ip'
                ? m.subscription_onboarding_host_kind_ip()
                : m.subscription_onboarding_host_kind_domain(),
            status: addressResult.status,
            latency: addressResult.latency_ms,
          }),
        });
      } catch (error) {
        updateStep('address-check', {
          status: 'failure',
          detail: formatProbeError(error),
        });
        skipSteps('stability-check', 'direct-import');
        await finishFailure(
          m.subscription_onboarding_failure_unreachable(),
          originalProxy,
          originalTun,
        );
        return;
      }

      updateStep('stability-check', { status: 'running' });
      const samples: NetworkProbeResult[] = [];

      try {
        for (let attempt = 0; attempt < STABILITY_ATTEMPTS; attempt += 1) {
          samples.push(await probe(target));
        }

        const latencies = samples.map((sample) => sample.latency_ms);
        const minimum = Math.min(...latencies);
        const maximum = Math.max(...latencies);
        const average = Math.round(
          latencies.reduce((total, latency) => total + latency, 0) /
            latencies.length,
        );

        if (maximum - minimum > STABILITY_LATENCY_SPREAD_MS) {
          throw new Error(
            m.subscription_onboarding_stability_spread_too_large({
              minimum,
              maximum,
            }),
          );
        }

        updateStep('stability-check', {
          status: 'success',
          detail: m.subscription_onboarding_stability_success({
            attempts: samples.length,
            average,
          }),
        });
      } catch (error) {
        updateStep('stability-check', {
          status: 'failure',
          detail: formatProbeError(error),
        });
        skipSteps('direct-import');
        await finishFailure(
          m.subscription_onboarding_failure_unstable(),
          originalProxy,
          originalTun,
        );
        return;
      }

      updateStep('direct-import', { status: 'running' });

      try {
        await create.mutateAsync({
          type: 'url',
          data: {
            url: target,
            option: null,
            mode: 'direct',
          },
        });
        let restorationWarning = '';

        if (originalTun) {
          try {
            await tunMode.upsert(true);
          } catch (error) {
            restorationWarning = ` ${m.subscription_onboarding_restore_tun_warning(
              {
                error: formatError(error),
              },
            )}`;
          }
        }

        updateStep('direct-import', {
          status: 'success',
          detail: m.subscription_onboarding_direct_import_success(),
        });
        updateStep('network-setup', {
          status: 'success',
          detail: restorationWarning
            ? `${m.subscription_onboarding_network_setup_direct_success()} ${restorationWarning}`
            : m.subscription_onboarding_network_setup_direct_success(),
        });
        setResult({
          status: 'success',
          detail: m.subscription_onboarding_import_success_direct({
            warning: restorationWarning,
          }),
        });
      } catch (error) {
        updateStep('direct-import', {
          status: 'failure',
          detail: formatError(error),
        });
        await finishFailure(
          m.subscription_onboarding_failure_direct(),
          originalProxy,
          originalTun,
        );
      }
    } finally {
      setIsRunning(false);
    }
  });

  const handleEnableSystemProxy = useLockFn(async () => {
    try {
      await systemProxy.upsert(true);
      setProxyDecision('enabled');
      setResult({
        status: 'success',
        detail: m.subscription_onboarding_system_proxy_enabled(),
      });
    } catch (error) {
      setProxyDecision('failure');
      setResult({
        status: 'failure',
        detail: m.subscription_onboarding_system_proxy_enable_failed({
          error: formatError(error),
        }),
      });
    }
  });

  const handleSkipSystemProxy = () => {
    setProxyDecision('skipped');
    setResult({
      status: 'success',
      detail: m.subscription_onboarding_system_proxy_skipped(),
    });
  };

  return (
    <BasePage title={m.subscription_onboarding_title()}>
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-4">
        <Card variant="outlined">
          <CardContent className="flex flex-col gap-3">
            <Typography variant="h6">
              {m.subscription_onboarding_heading()}
            </Typography>
            <Typography color="text.secondary" variant="body2">
              {m.subscription_onboarding_description()}
            </Typography>
            <QuickImport
              value={url}
              loading={isRunning}
              disabled={isRunning}
              onChange={setUrl}
              onImport={handleImport}
            />
          </CardContent>
        </Card>

        <Stack spacing={2} data-slot="best-effort-import-steps">
          {stepDefinitions.map((step, index) => (
            <StepCard
              key={step.id}
              index={index}
              id={step.id}
              title={step.title}
              description={step.description}
              icon={step.icon}
              state={steps[step.id]}
            />
          ))}
        </Stack>

        {result && (
          <Card variant="outlined" data-slot="best-effort-import-result">
            <CardContent>
              <Alert
                severity={result.status === 'success' ? 'success' : 'error'}
                icon={
                  result.status === 'success' ? (
                    <CheckCircleOutlined />
                  ) : (
                    <ErrorOutlined />
                  )
                }
              >
                {result.detail}
              </Alert>

              {result.status === 'success' && !isRunning && (
                <div className="mt-4 flex flex-wrap justify-end gap-2">
                  <Button
                    data-slot="best-effort-enable-proxy"
                    variant="contained"
                    disabled={proxyDecision === 'enabled'}
                    onClick={handleEnableSystemProxy}
                  >
                    {proxyDecision === 'enabled'
                      ? m.subscription_onboarding_enable_proxy_done()
                      : m.subscription_onboarding_enable_proxy()}
                  </Button>
                  <Button
                    data-slot="best-effort-skip-proxy"
                    variant="outlined"
                    disabled={proxyDecision === 'skipped'}
                    onClick={handleSkipSystemProxy}
                  >
                    {m.subscription_onboarding_skip_proxy()}
                  </Button>
                </div>
              )}

              {result.status === 'failure' && !isRunning && (
                <div className="mt-4 flex flex-wrap justify-end gap-2">
                  <Button
                    data-slot="best-effort-retry"
                    variant="contained"
                    disabled={!url.trim()}
                    onClick={() => void handleImport(url)}
                  >
                    {m.subscription_onboarding_retry()}
                  </Button>
                  <Button
                    variant="outlined"
                    onClick={() => {
                      setUrl('');
                      setResult(null);
                      setProxyDecision('idle');
                      setSteps(createInitialSteps());
                    }}
                  >
                    {m.subscription_onboarding_edit_url()}
                  </Button>
                </div>
              )}
            </CardContent>
          </Card>
        )}
      </div>
    </BasePage>
  );
}

export default BestEffortSubscriptionImport;
