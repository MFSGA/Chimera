import { useSetting } from '@chimera/interface';
import { BaseCard, BaseItem, Expand } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, TextField } from '@mui/material';
import { useEffect, useState } from 'react';
import * as m from '@/paraglide/messages';

const MaxLogFiles = () => {
  const maxLogFiles = useSetting('max_log_files');

  const value = maxLogFiles.value ?? 7;
  const [inputValue, setInputValue] = useState(String(value));

  useEffect(() => {
    setInputValue(String(value));
  }, [value]);

  const parsed = Number(inputValue);
  const invalid = !Number.isInteger(parsed) || parsed < 1;
  const changed = inputValue !== String(value);

  return (
    <>
      <BaseItem title={m.settings_nyanpasu_max_log_files_label()}>
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
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => maxLogFiles.upsert(parsed)}
            disabled={invalid}
          >
            {m.common_apply()}
          </Button>
        </div>
      </Expand>
    </>
  );
};

export const SettingChimeraTasks = () => {
  return (
    <BaseCard label={'Tasks'}>
      <List disablePadding>
        <MaxLogFiles />
      </List>
    </BaseCard>
  );
};

export default SettingChimeraTasks;
