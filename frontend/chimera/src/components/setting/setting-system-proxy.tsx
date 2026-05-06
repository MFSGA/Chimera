import {
  useSetting,
  useSystemProxy as useSystemProxyQuery,
} from '@chimera/interface';
import {
  BaseCard,
  BaseItem,
  cn,
  Expand,
  ExpandMore,
  SwitchItem,
} from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import {
  Grid,
  List,
  ListItem,
  Button as MuiButton,
  TextField,
} from '@mui/material';
import NetworkPing from '~icons/material-symbols/network-ping-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, type ButtonProps } from '@/components/ui/button';
import { CircularProgress } from '@/components/ui/progress';
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings';

const DEFAULT_BYPASS =
  'localhost;127.;192.168.;10.;' +
  '172.16.;172.17.;172.18.;172.19.;172.20.;172.21.;172.22.;172.23.;' +
  '172.24.;172.25.;172.26.;172.27.;172.28.;172.29.;172.30.;172.31.*';

const ProxyButton = ({
  className,
  isActive,
  loading,
  children,
  ...props
}: ButtonProps & {
  isActive?: boolean;
}) => {
  return (
    <Button
      className={cn(
        'group h-16 rounded-3xl font-bold text-nowrap',
        'flex items-center justify-between gap-2',
        'data-[active=false]:bg-white dark:data-[active=false]:bg-black',
        className,
      )}
      data-active={String(Boolean(isActive))}
      data-loading={String(Boolean(loading))}
      disabled={loading}
      variant="fab"
      {...props}
    >
      <div className="flex items-center gap-3 [&_svg]:size-7">{children}</div>

      {loading && (
        <CircularProgress
          className={cn(
            'size-6 transition-opacity',
            'group-data-[loading=false]:opacity-0 group-data-[loading=true]:opacity-100',
          )}
          indeterminate
        />
      )}
    </Button>
  );
};

const TunModeButton = (props: Omit<ButtonProps, 'children' | 'loading'>) => {
  const { t } = useTranslation();
  const { execute, isPending, isActive } = useTunMode();

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <SettingsEthernet />
      <span>{t('TUN Mode')}</span>
    </ProxyButton>
  );
};

const SystemProxyButton = (
  props: Omit<ButtonProps, 'children' | 'loading'>,
) => {
  const { t } = useTranslation();
  const { execute, isPending, isActive } = useSystemProxy();

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <NetworkPing />
      <span>{t('System Proxy')}</span>
    </ProxyButton>
  );
};

const ProxyGuardSwitch = () => {
  const { t } = useTranslation();
  const proxyGuard = useSetting('enable_proxy_guard');

  return (
    <SwitchItem
      label={t('Proxy Guard')}
      checked={Boolean(proxyGuard.value)}
      onChange={() => proxyGuard.upsert(!proxyGuard.value)}
    />
  );
};

const ProxyGuardInterval = () => {
  const { t } = useTranslation();
  const proxyGuardInterval = useSetting('proxy_guard_interval');

  const value = proxyGuardInterval.value ?? 1;
  const [inputValue, setInputValue] = useState(String(value));

  useEffect(() => {
    setInputValue(String(value));
  }, [value]);

  const parsed = Number(inputValue);
  const invalid = !Number.isInteger(parsed) || parsed < 1;
  const changed = inputValue !== String(value);

  return (
    <>
      <BaseItem title={t('Guard Interval')}>
        <TextField
          size="small"
          type="number"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 128 }}
          slotProps={{ htmlInput: { min: 1 } }}
        />
      </BaseItem>

      <Expand open={changed}>
        <div className="flex justify-end">
          <MuiButton
            variant="contained"
            startIcon={<Done />}
            onClick={() => proxyGuardInterval.upsert(parsed)}
            disabled={invalid}
          >
            {t('Apply')}
          </MuiButton>
        </div>
      </Expand>
    </>
  );
};

const SystemProxyBypass = () => {
  const { t } = useTranslation();
  const proxyBypass = useSetting('system_proxy_bypass');

  const value = proxyBypass.value ?? '';
  const [inputValue, setInputValue] = useState(value);

  useEffect(() => {
    setInputValue(value);
  }, [value]);

  return (
    <>
      <BaseItem title={t('Proxy Bypass')}>
        <TextField
          size="small"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 220 }}
          slotProps={{ htmlInput: { 'aria-autocomplete': 'none' } }}
        />
      </BaseItem>

      <Expand open={inputValue !== value}>
        <div className="flex justify-end">
          <MuiButton
            variant="contained"
            startIcon={<Done />}
            onClick={() => proxyBypass.upsert(inputValue || DEFAULT_BYPASS)}
          >
            {t('Apply')}
          </MuiButton>
        </div>
      </Expand>
    </>
  );
};

const CurrentSystemProxy = () => {
  const { t } = useTranslation();
  const { data } = useSystemProxyQuery();
  const entries = Object.entries(data ?? {});

  return (
    <ListItem
      className="!w-full !flex-col !items-start select-text"
      sx={{ pl: 0, pr: 0 }}
    >
      <div className="text-base leading-10">{t('Current System Proxy')}</div>

      {entries.length > 0 ? (
        entries.map(([key, value], index) => (
          <div key={index} className="flex w-full leading-8">
            <div className="w-28 shrink-0 capitalize">{key}:</div>

            <div className="min-w-0 flex-1 break-all">{String(value)}</div>
          </div>
        ))
      ) : (
        <div className="leading-8">{t('No System Proxy')}</div>
      )}
    </ListItem>
  );
};

export const SettingSystemProxy = () => {
  const { t } = useTranslation();
  const [expand, setExpand] = useState(false);

  return (
    <BaseCard
      label={t('System Setting')}
      labelChildren={
        <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
      }
    >
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <TunModeButton />
        </Grid>

        <Grid size={{ xs: 6 }}>
          <SystemProxyButton />
        </Grid>
      </Grid>

      <Expand open={expand}>
        <List disablePadding sx={{ pt: 1 }}>
          <ProxyGuardSwitch />

          <ProxyGuardInterval />

          <SystemProxyBypass />

          <CurrentSystemProxy />
        </List>
      </Expand>
    </BaseCard>
  );
};

export default SettingSystemProxy;
