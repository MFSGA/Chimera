import { useAgentBridge } from '@chimera/interface';
import {
  ContentCopyRounded,
  HubRounded,
  PlayArrowRounded,
  StopRounded,
  VpnKeyRounded,
} from '@mui/icons-material';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { useEffect, useReducer } from 'react';
import { Notice } from '@/components/base';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentAgentError } from '../model/presenter';
import {
  reduceBridgeToken,
  scheduleClipboardValueClear,
  scheduleTokenExpiry,
} from './bridge-token-lifecycle';

export function BridgeCard() {
  const bridge = useAgentBridge();
  const [token, dispatchToken] = useReducer(reduceBridgeToken, null);
  const status = bridge.status.data;
  const running = status?.running === true;

  useEffect(() => {
    if (!token) return;
    return scheduleTokenExpiry(token, (expiredToken) => {
      dispatchToken({ type: 'expired', token: expiredToken });
    });
  }, [token]);

  useEffect(() => {
    dispatchToken({ type: 'running_changed', running });
  }, [running]);

  const startBridge = async () => {
    try {
      const result = await bridge.start.mutateAsync();
      if (!result) return;
      dispatchToken({ type: 'started', token: result.token });
      Notice.success(m.agent_bridge_started());
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const stopBridge = async () => {
    try {
      await bridge.stop.mutateAsync();
      dispatchToken({ type: 'running_changed', running: false });
      Notice.success(m.agent_bridge_stopped_notice());
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const copyAddress = async () => {
    if (!status?.base_url) return;
    await copyValue(status.base_url, m.agent_bridge_address_copied());
  };

  const copyToken = async () => {
    if (!token) return;
    const copied = await copyValue(token, m.agent_bridge_token_copied());
    if (copied) {
      dispatchToken({ type: 'copied', token });
      scheduleClipboardValueClear(token, { readText, writeText });
    }
  };

  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        <HubRounded />
        {m.agent_bridge_title()}
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_bridge_description()}
        </p>

        <div className="bg-surface-variant/25 flex flex-col gap-3 rounded-2xl p-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="text-sm font-medium">
                {running ? m.agent_bridge_running() : m.agent_bridge_stopped()}
              </p>
              {status?.base_url && (
                <code className="text-on-surface-variant text-xs break-all">
                  {status.base_url}
                </code>
              )}
            </div>
            <div className="flex flex-wrap gap-2">
              {status?.base_url && (
                <Button variant="stroked" onClick={() => void copyAddress()}>
                  <ContentCopyRounded />
                  {m.agent_bridge_copy_address()}
                </Button>
              )}
              {running ? (
                <Button
                  loading={bridge.stop.isPending}
                  variant="stroked"
                  onClick={() => void stopBridge()}
                >
                  <StopRounded />
                  {m.agent_bridge_stop()}
                </Button>
              ) : (
                <Button
                  loading={bridge.start.isPending}
                  variant="flat"
                  onClick={() => void startBridge()}
                >
                  <PlayArrowRounded />
                  {m.agent_bridge_start()}
                </Button>
              )}
            </div>
          </div>

          {token && (
            <div className="border-outline-variant flex flex-wrap items-center justify-between gap-3 rounded-xl border p-3">
              <div className="flex items-center gap-2 text-sm">
                <VpnKeyRounded className="size-5" />
                {m.agent_bridge_token_ready()}
              </div>
              <Button variant="stroked" onClick={() => void copyToken()}>
                <ContentCopyRounded />
                {m.agent_bridge_copy_token()}
              </Button>
            </div>
          )}
        </div>

        <div>
          <p className="mb-2 text-sm font-medium">{m.agent_bridge_tools()}</p>
          <div className="flex flex-col gap-2">
            {bridge.manifest.data?.tools.map((tool) => (
              <div
                className="border-outline-variant flex flex-wrap items-center justify-between gap-2 rounded-xl border px-3 py-2"
                key={`${tool.name}:${tool.version}`}
              >
                <div>
                  <code className="text-sm">{tool.name}</code>
                  <p className="text-on-surface-variant text-xs">
                    {tool.description}
                  </p>
                </div>
                {tool.read_only && (
                  <span className="bg-secondary-container text-on-secondary-container rounded-full px-2 py-1 text-xs">
                    {m.agent_bridge_read_only()}
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

async function copyValue(value: string, successMessage: string) {
  try {
    await writeText(value);
    Notice.success(successMessage);
    return true;
  } catch {
    Notice.error(m.common_failed_copy_to_clipboard());
    return false;
  }
}
