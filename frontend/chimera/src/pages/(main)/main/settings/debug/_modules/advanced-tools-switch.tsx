import { Switch } from '@/components/ui/switch';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card';
import { useDebugContext } from './debug-provider';

export default function AdvancedToolsSwitch() {
  const { advancedTools, setAdvancedTools } = useDebugContext();

  return (
    <ItemContainer data-slot="advanced-tools-switch-container">
      <ItemLabel>
        <ItemLabelText>Advanced Tools</ItemLabelText>
      </ItemLabel>
      <Switch checked={advancedTools} onCheckedChange={setAdvancedTools} />
    </ItemContainer>
  );
}
