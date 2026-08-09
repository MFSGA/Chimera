import { openThat, useProfile, useSetting } from '@chimera/interface';
import { cn } from '@chimera/ui';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded';
import { useMemo, type PropsWithChildren } from 'react';
import CLASH_FIELD from '@/assets/json/clash-field.json';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '@/components/settings/settings-card';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Switch } from '@/components/ui/switch';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';

type FieldState = {
  url?: string;
  enabled: boolean;
};

const FieldButton = ({
  label,
  state,
  disabled,
}: {
  label: string;
  state: FieldState;
  disabled: boolean;
}) => {
  const { query, setValidFields } = useProfile();

  const handleToggle = async () => {
    const current = query.data?.valid ?? [];
    const next = state.enabled
      ? current.filter((item) => item !== label)
      : [...current, label];
    await setValidFields.mutateAsync(next);
  };

  return (
    <Button
      data-enabled={String(state.enabled)}
      className={cn(
        'flex h-12 items-center justify-between gap-2 rounded-2xl pr-3',
        'data-[enabled=true]:bg-primary-container',
        'data-[enabled=false]:bg-primary-container/50',
        'dark:data-[enabled=true]:bg-surface-variant',
        'dark:data-[enabled=false]:bg-surface-variant/10',
      )}
      disabled={disabled}
      loading={setValidFields.isPending}
      onClick={() => void handleToggle()}
    >
      <TextMarquee className="w-full min-w-0 text-left text-sm">
        {label}
      </TextMarquee>

      {state.url && (
        <Button
          variant="stroked"
          className="size-6"
          icon
          aria-label={m.common_open()}
          onClick={(event) => {
            event.stopPropagation();
            void openThat(state.url!);
          }}
          asChild
        >
          <span>
            <OpenInNewRounded className="size-3" />
          </span>
        </Button>
      )}
    </Button>
  );
};

const FieldGroupButton = ({
  group,
  fields,
}: PropsWithChildren<{
  group: string;
  fields: Record<string, FieldState>;
}>) => {
  const isControlField = ['default', 'handle'].includes(group);
  const enabledFields = Object.entries(fields)
    .filter(([, state]) => state.enabled)
    .map(([field]) => field);

  return (
    <Modal>
      <ModalTrigger asChild>
        <Button
          className={cn(
            'relative h-20 w-full min-w-0 rounded-3xl pr-8',
            'bg-primary-container dark:bg-surface-variant/30',
            'flex flex-col items-start justify-center gap-0.5',
          )}
        >
          <div className="text-base font-bold capitalize">{group}</div>
          <TextMarquee className="w-full min-w-0 text-left text-sm">
            {enabledFields.length > 0
              ? `Enabled: ${enabledFields.join(' ')}`
              : 'Enabled: -'}
          </TextMarquee>
          <ArrowForwardIosRounded className="absolute top-1/2 right-2 size-5 -translate-y-1/2" />
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle className="capitalize">{group}</ModalTitle>
            {isControlField && (
              <div className="text-on-surface-variant text-sm">
                {m.settings_clash_fields_control_fields_label()}
              </div>
            )}
          </CardHeader>
          <CardContent asChild>
            <ScrollArea className="max-h-[80dvh]">
              <div className="grid grid-cols-2 gap-2">
                {Object.entries(fields).map(([field, state]) => (
                  <FieldButton
                    key={field}
                    label={field}
                    state={state}
                    disabled={isControlField}
                  />
                ))}
              </div>
            </ScrollArea>
          </CardContent>
          <CardFooter>
            <ModalClose>{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
};

export const FieldFilterSwitch = () => {
  const setting = useSetting('enable_clash_fields');

  return (
    <ItemContainer data-slot="field-filter-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_field_filter_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
        checked={Boolean(setting.value)}
        loading={setting.isPending}
        onCheckedChange={(checked) => void setting.upsert(checked)}
      />
    </ItemContainer>
  );
};

export const FieldFilterCard = () => {
  const { query } = useProfile();
  const enabledFields = useMemo(
    () => [
      ...Object.keys(CLASH_FIELD.default),
      ...Object.keys(CLASH_FIELD.handle),
      ...(query.data?.valid ?? []),
    ],
    [query.data?.valid],
  );

  return (
    <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
      {Object.entries(CLASH_FIELD).map(([group, values]) => {
        const fields = Object.fromEntries(
          Object.entries(values).map(([field, url]) => [
            field,
            { url, enabled: enabledFields.includes(field) },
          ]),
        );

        return <FieldGroupButton key={group} group={group} fields={fields} />;
      })}
    </div>
  );
};
