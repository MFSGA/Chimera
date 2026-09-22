import { ClashManifest } from 'types';
import versionManifest from '../../manifest/version.json';

const CLASH_RS_MIRROR_TAG = 'deps-clash-rs-0.10.8';
const CLASH_RS_MIRROR_URL = `https://github.com/MFSGA/Chimera_Service/releases/download/${CLASH_RS_MIRROR_TAG}`;

export const CLASH_RS_MANIFEST: ClashManifest = {
  URL_PREFIX: CLASH_RS_MIRROR_URL,
  VERSION: versionManifest.latest.clash_rs,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs,
};

export const CLASH_RS_ALPHA_MANIFEST: ClashManifest = {
  URL_PREFIX: CLASH_RS_MIRROR_URL,
  VERSION: versionManifest.latest.clash_rs_alpha,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha,
};
