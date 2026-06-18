import { alpha } from '@chimera/ui';
import { TextField, type FilledInputProps } from '@mui/material';
import * as m from '@/paraglide/messages';
import { useLogContext } from './log-provider';

export const LogFilter = () => {
  const { filterText, setFilterText } = useLogContext();

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
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
      placeholder={m.logs_filter_placeholder()}
      onChange={(e) => setFilterText(e.target.value)}
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      slotProps={{
        input: inputProps,
      }}
    />
  );
};
