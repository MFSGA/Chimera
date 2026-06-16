import { alpha } from '@chimera/ui';
import { FilledInputProps, TextField, TextFieldProps } from '@mui/material';
import * as m from '@/paraglide/messages';

export const HeaderSearch = (props: TextFieldProps) => {
  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),

      '&::before': {
        display: 'none',
      },

      '&::after': {
        display: 'none',
      },
    }),
  };

  return (
    <TextField
      autoComplete="off"
      spellCheck="false"
      hiddenLabel
      placeholder={m.connections_search_placeholder()}
      variant="filled"
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      slotProps={{
        input: inputProps,
      }}
      {...props}
    />
  );
};

export default HeaderSearch;
