import { ClashManifest } from 'types';
import versionManifest from '../../manifest/version.json';

export const CHIMERA_CLIENT_MANIFEST: ClashManifest = {
  URL_PREFIX: 'https://github.com/MFSGA/Chimera_Client/releases/download/',
  VERSION: versionManifest.latest.chimera_client,
  ARCH_MAPPING: versionManifest.arch_template.chimera_client,
};
