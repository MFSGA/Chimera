import { ClashManifest } from 'types';
import versionManifest from '../../manifest/version.json';
import { CLASH_RS_MIRROR_URL } from '../utils/clash-rs-mirror';

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
