import { alpha } from '@chimera/ui';
import { Button, Menu, MenuItem } from '@mui/material';
import { useAtom } from 'jotai';
import { memo, useState } from 'react';
import * as m from '@/paraglide/messages';
import { proxyGroupSortAtom } from '@/store';

export const SortSelector = memo(function SortSelector() {
  const [proxyGroupSort, setProxyGroupSort] = useAtom(proxyGroupSortAtom);

  type SortType = typeof proxyGroupSort;

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const handleClick = (sort: SortType) => {
    setAnchorEl(null);
    setProxyGroupSort(sort);
  };

  const tmaps: Record<string, () => string> = {
    default: m.proxies_sort_default,
    delay: m.proxies_sort_latency,
    name: m.proxies_sort_name,
  };

  return (
    <>
      <Button
        size="small"
        className="!px-2"
        sx={(theme) => ({
          textTransform: 'none',
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
        })}
        onClick={(e) => setAnchorEl(e.currentTarget)}
      >
        {tmaps[proxyGroupSort]()}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(tmaps).map(([key, value], index) => {
          return (
            <MenuItem key={index} onClick={() => handleClick(key as SortType)}>
              {value()}
            </MenuItem>
          );
        })}
      </Menu>
    </>
  );
});

export default SortSelector;
