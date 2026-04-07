import { useClashRules } from '@chimera/interface';
import { alpha, BasePage } from '@chimera/ui';
import { TextField, type FilledInputProps } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useDebounceEffect } from 'ahooks';
import { useSetAtom } from 'jotai';
import { lazy, Suspense, useRef, useState, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { atomRulePage } from '@/components/rules/modules/store';

export const Route = createFileRoute('/(legacy)/rules')({
  component: RulesPage,
});

function RulesPage() {
  const { t } = useTranslation();
  const { data } = useClashRules();
  const [filterText, setFilterText] = useState('');
  const setRule = useSetAtom(atomRulePage);
  const viewportRef = useRef<HTMLDivElement>(null);

  useDebounceEffect(
    () => {
      const search = filterText.trim().toLowerCase();

      setRule({
        data: data?.rules.filter((each) => {
          if (!search) {
            return true;
          }

          return [each.type, each.payload, each.proxy].some((value) =>
            value?.toLowerCase().includes(search),
          );
        }),
        scrollRef: viewportRef as RefObject<HTMLElement>,
        searchText: filterText,
      });

      viewportRef.current?.scrollTo({
        top: 0,
      });
    },
    [data, viewportRef.current, filterText],
    { wait: 150 },
  );

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
      fieldset: {
        border: 'none',
      },
    }),
  };

  const Component = lazy(() => import('@/components/rules/rule-page'));

  return (
    <BasePage
      full
      title={t('Rules')}
      header={
        <TextField
          hiddenLabel
          autoComplete="off"
          spellCheck="false"
          value={filterText}
          placeholder={t('Filter conditions')}
          onChange={(e) => setFilterText(e.target.value)}
          className="!pb-0"
          sx={{ input: { py: 1, fontSize: 14 } }}
          slotProps={{
            input: inputProps,
          }}
        />
      }
      viewportRef={viewportRef}
    >
      <Suspense fallback={null}>
        <Component />
      </Suspense>
    </BasePage>
  );
}
