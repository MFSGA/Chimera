import { ClashManifest } from 'types';
import versionManifest from '../../manifest/version.json';

export const CHIMERA_CLIENT_ALPHA_MANIFEST: ClashManifest = {
  URL_PREFIX:
    'https://github.com/MFSGA/Chimera_Client/releases/download/latest',
  VERSION: versionManifest.latest.chimera_client_alpha,
  ARCH_MAPPING: versionManifest.arch_template.chimera_client_alpha,
};
