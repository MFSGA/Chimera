import { alpha } from '@chimera/ui';
import { TextField, type FilledInputProps } from '@mui/material';
import { useTranslation } from 'react-i18next';
import { useLogContext } from './log-provider';

export const LogFilter = () => {
  const { t } = useTranslation();
  const { filterText, setFilterText } = useLogContext();

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      height: 36,
      borderRadius: 10,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
      fieldset: {
        border: 'none',
      },
    }),
  };

  return (
    <TextField
      hiddenLabel
      autoComplete="off"
      spellCheck="false"
      value={filterText}
      placeholder={t('Filter conditions')}
      onChange={(event) => setFilterText(event.target.value)}
      className="!pb-0"
      sx={{
        input: { py: 1, px: 1.5, fontSize: 14 },
      }}
      slotProps={{
        input: inputProps,
      }}
    />
  );
};
