import path from 'node:path';
import fs from 'fs-extra';

const workspaceRoot = process.cwd();
const runtimeRoot = path.join(workspaceRoot, 'backend/chimera-runtime');

const readPackageVersion = async (manifestPath: string): Promise<string> => {
  const manifest = await fs.readFile(manifestPath, 'utf8');
  const match = manifest.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms);

  if (!match) {
    throw new Error(`failed to parse package version from ${manifestPath}`);
  }

  return match[1];
};

const ipcManifest = path.join(runtimeRoot, 'chimera_ipc/Cargo.toml');
const serviceManifest = path.join(runtimeRoot, 'chimera_service/Cargo.toml');

const [ipcVersion, serviceVersion] = await Promise.all([
  readPackageVersion(ipcManifest),
  readPackageVersion(serviceManifest),
]);

if (ipcVersion !== serviceVersion) {
  throw new Error(
    `chimera runtime version mismatch: chimera-ipc=${ipcVersion}, chimera-service=${serviceVersion}`,
  );
}

console.log(`chimera runtime versions aligned at v${serviceVersion}`);
