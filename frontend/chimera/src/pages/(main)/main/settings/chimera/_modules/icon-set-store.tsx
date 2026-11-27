import {
  mutations,
  queries,
  trayIconUrl,
  type TrayIcon,
} from '@chimera/interface';
import type { LazyImageProps } from '@chimera/ui/lazy-image';
import { createContext, useContext, type ComponentType } from 'react';
import { TrayImage } from '@/components/ui/image';
import { m } from '@/paraglide/messages';

/**
 * Where an icon set with a normal, a TUN and a system proxy icon is stored:
 * the tray's, or the statistic widget's own. The icon set editor reads and
 * writes through it, so one editor serves both.
 */
export interface IconSetStore {
  useIcon: (mode: TrayIcon, version?: number) => string;
  Image: ComponentType<
    Omit<LazyImageProps, 'src'> & { mode: TrayIcon; version?: number }
  >;
  isSetQuery: typeof queries.isTrayIconSet;
  setIcon: typeof mutations.setTrayIcon;
  setIconFromBytes: typeof mutations.setTrayIconFromBytes;
  /** Identifies the icon set in preset detection. */
  kind: 'tray' | 'widget';
  isSetKey: string;
  /** Whether the preset cards are offered; otherwise each state is edited on its own. */
  presets: boolean;
  messages: {
    label: () => string;
    presetTitle: () => string;
    setSuccess: () => string;
    setFailed: () => string;
  };
}

export const TRAY_ICON_STORE: IconSetStore = {
  useIcon: trayIconUrl,
  Image: TrayImage,
  isSetQuery: (mode) => queries.isTrayIconSet(mode),
  setIcon: mutations.setTrayIcon,
  setIconFromBytes: mutations.setTrayIconFromBytes,
  kind: 'tray',
  isSetKey: 'isTrayIconSet',
  presets: false,
  messages: {
    label: m.settings_chimera_tray_icon,
    presetTitle: m.settings_chimera_tray_icon_preset_title,
    setSuccess: m.settings_chimera_tray_icon_set_success,
    setFailed: m.settings_chimera_tray_icon_set_failed,
  },
};

const IconSetStoreContext = createContext<IconSetStore>(TRAY_ICON_STORE);

export const IconSetStoreProvider = IconSetStoreContext.Provider;

export const useIconSetStore = () => useContext(IconSetStoreContext);
