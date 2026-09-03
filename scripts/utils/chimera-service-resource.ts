import path from 'node:path';
import fs from 'fs-extra';
import { BinInfo } from '../types';
import { cwd } from './env';

const CHIMERA_SERVICE_REPO = 'MFSGA/Chimera_Service';
const CHIMERA_SERVICE_NAME = 'chimera-service';
const CHIMERA_SERVICE_MANIFEST = path.join(
  cwd,
  'backend/chimera-runtime/chimera_service/Cargo.toml',
);

// Keep the sidecar release aligned with the IPC source selected by the
// `backend/chimera-runtime` gitlink. Chimera Service releases are tagged
// `v<chimera-service crate version>`.
export const getChimeraServiceVersion = async (): Promise<string> => {
  const manifest = await fs.readFile(CHIMERA_SERVICE_MANIFEST, 'utf8');
  const match = manifest.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms);

  if (!match) {
    throw new Error(
      `failed to parse chimera-service version from ${CHIMERA_SERVICE_MANIFEST}`,
    );
  }

  return `v${match[1]}`;
};

export const getChimeraServiceInfo = async ({
  sidecarHost,
}: {
  sidecarHost: string;
}): Promise<BinInfo> => {
  const isWin = sidecarHost.includes('windows');
  const urlExt = isWin ? 'zip' : 'tar.gz';
  const version = await getChimeraServiceVersion();
  const downloadURL = `https://github.com/${CHIMERA_SERVICE_REPO}/releases/download/${version}/${CHIMERA_SERVICE_NAME}-${sidecarHost}.${urlExt}`;
  const exeFile = `${CHIMERA_SERVICE_NAME}${isWin ? '.exe' : ''}`;
  const tmpFile = `${CHIMERA_SERVICE_NAME}-${sidecarHost}.${urlExt}`;
  const targetFile = `${CHIMERA_SERVICE_NAME}-${sidecarHost}${isWin ? '.exe' : ''}`;

  return {
    name: CHIMERA_SERVICE_NAME,
    version,
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};
