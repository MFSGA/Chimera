import { NodeArch } from './manifest';

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
}
