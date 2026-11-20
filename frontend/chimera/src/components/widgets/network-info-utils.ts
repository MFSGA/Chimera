/** Preserve the IPv4/IPv6 separator layout without revealing address digits. */
export const maskAddress = (address: string) =>
  address.replace(/[0-9a-f]/gi, '•');
