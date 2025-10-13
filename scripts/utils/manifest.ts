import { SupportedArch } from '../types/index';

export type NodeArch = NodeJS.Architecture | 'armel';

export type ArchMapping = { [key in SupportedArch]: string };
