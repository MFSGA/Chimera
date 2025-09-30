export const alpha = (color: string, alpha: number) => {
  return `color-mix(in srgb, ${color} ${(alpha * 100).toFixed(2)}%, transparent ${((1 - alpha) * 100).toFixed(2)}%)`;
};
