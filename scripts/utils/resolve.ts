import { BinInfo } from 'types';
import { resolveSidecar } from './download';
import { NodeArch } from './manifest';
import {
  getClashMetaInfo,
  getClashRustAlphaInfo,
  getClashRustInfo,
} from './resource';

export class Resolve {
  private infoOption: {
    platform: NodeJS.Platform;
    arch: NodeArch;
    sidecarHost: string;
  };

  constructor(
    private readonly options: {
      force?: boolean;
      platform: NodeJS.Platform;
      arch: NodeArch;
      sidecarHost: string;
    },
  ) {
    this.infoOption = {
      platform: this.options.platform,
      arch: this.options.arch,
      sidecarHost: this.options.sidecarHost,
    };
  }

  private sidecar(binInfo: BinInfo | PromiseLike<BinInfo>) {
    return resolveSidecar(binInfo, this.options.platform, {
      force: this.options.force,
    });
  }

  public async clashMeta() {
    return await this.sidecar(getClashMetaInfo(this.infoOption));
  }

  public async clashRust() {
    return await this.sidecar(getClashRustInfo(this.infoOption));
  }

  public async clashRustAlpha() {
    return await this.sidecar(getClashRustAlphaInfo(this.infoOption));
  }
}
