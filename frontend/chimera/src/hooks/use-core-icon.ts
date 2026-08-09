import type { ClashCore } from '@chimera/interface';
import ClashRs from '@/assets/image/core/clash-rs.png';
import ClashMeta from '@/assets/image/core/clash.meta.png';
import Clash from '@/assets/image/core/clash.png';

/** Resolve the current core key to the matching bundled icon. */
export default function useCoreIcon(core?: ClashCore | null) {
  switch (core) {
    case 'clash':
    case 'clash-premium':
      return Clash;
    case 'clash-rs':
    case 'clash-rs-alpha':
    case 'chimera-client':
    case 'chimera_client':
      return ClashRs;
    default:
      return ClashMeta;
  }
}
