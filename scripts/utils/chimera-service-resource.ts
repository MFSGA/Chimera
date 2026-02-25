import { fetch, type RequestInit } from 'undici';
import { BinInfo } from '../types';
import { getProxyAgent } from './';
import { SIDECAR_HOST } from './consts';
import { consola } from './logger';

const CHIMERA_SERVICE_REPO = 'MFSGA/Chimera_Service';
const CHIMERA_SERVICE_NAME = 'chimera-service';

export const getChimeraServiceLatestVersion = async () => {
  try {
    const opts = {} as Partial<RequestInit>;

    const httpProxy = getProxyAgent();
    if (httpProxy) {
      opts.dispatcher = httpProxy;
    }

    const url = new URL('https://github.com');
    url.pathname = `/${CHIMERA_SERVICE_REPO}/releases/latest`;
    const response = await fetch(url, {
      method: 'GET',
      redirect: 'manual',
      ...opts,
    });

    const location = response.headers.get('location');
    if (!location) {
      throw new Error('Cannot find location from the response header');
    }

    const tag = location.split('/').pop();
    if (!tag) {
      throw new Error('Cannot find tag from the location');
    }

    consola.info(`Chimera Service latest release version: ${tag}`);

    return tag.trim();
  } catch (error) {
    console.error('Error fetching latest release version:', error);

    process.exit(1);
  }
};

export const getChimeraServiceInfo = async ({
  sidecarHost,
}: {
  sidecarHost: string;
}): Promise<BinInfo> => {
  const isWin = SIDECAR_HOST?.includes('windows');
  const urlExt = isWin ? 'zip' : 'tar.gz';
  const version = await getChimeraServiceLatestVersion();
  const downloadURL = `https://github.com/${CHIMERA_SERVICE_REPO}/releases/download/${version}/${CHIMERA_SERVICE_NAME}-${sidecarHost}.${urlExt}`;
  const exeFile = `${CHIMERA_SERVICE_NAME}${isWin ? '.exe' : ''}`;
  const tmpFile = `${CHIMERA_SERVICE_NAME}-${sidecarHost}.${urlExt}`;
  const targetFile = `${CHIMERA_SERVICE_NAME}-${sidecarHost}${isWin ? '.exe' : ''}`;

  return {
    name: CHIMERA_SERVICE_NAME,
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};
