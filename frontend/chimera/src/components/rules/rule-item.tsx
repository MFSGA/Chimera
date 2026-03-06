import type { ClashRule } from '@chimera/interface';
import { Box, type SxProps, type Theme } from '@mui/material';

interface Props {
  index: number;
  value: ClashRule;
}

const COLOR = [
  (theme: Theme) => ({
    color: theme.vars.palette.primary.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.secondary.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.info.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.warning.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.success.main,
  }),
] satisfies SxProps<Theme>[];

const RuleItem = ({ index, value }: Props) => {
  const parseColorSx = (text: string): SxProps<Theme> => {
    const typeMap = {
      reject: ['REJECT', 'REJECT-DROP'],
      direct: ['DIRECT'],
    };

    if (typeMap.reject.includes(text)) {
      return (theme) => ({ color: theme.vars.palette.error.main });
    }

    if (typeMap.direct.includes(text)) {
      return (theme) => ({ color: theme.vars.palette.text.primary });
    }

    let sum = 0;
    for (let i = 0; i < text.length; i++) {
      sum += text.charCodeAt(i);
    }

    return COLOR[sum % COLOR.length];
  };

  return (
    <div className="flex p-2 pr-7 pl-7 select-text">
      <Box
        sx={(theme) => ({ color: theme.vars.palette.text.secondary })}
        className="min-w-14"
      >
        {index + 1}
      </Box>

      <div className="flex flex-col gap-1">
        <Box sx={(theme) => ({ color: theme.vars.palette.text.primary })}>
          {value.payload || '-'}
        </Box>

        <div className="flex gap-8">
          <div className="min-w-40 text-sm">{value.type}</div>

          <Box className="text-s text-sm" sx={parseColorSx(value.proxy)}>
            {value.proxy}
          </Box>
        </div>
      </div>
    </div>
  );
};

export default RuleItem;
