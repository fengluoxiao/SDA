/** React Native reports iOS versions as strings, including patch components. */
export function iosMajorVersion(version: unknown): number {
  if (typeof version === "number") return Number.isFinite(version) && version >= 0 ? Math.floor(version) : 0;
  if (typeof version !== "string") return 0;
  const major = /^(\d+)(?:\.|$)/.exec(version.trim());
  return major ? Number.parseInt(major[1]!, 10) : 0;
}
export function supportsIOSGlass(platform: string, version: unknown): boolean {
  return platform === "ios" && iosMajorVersion(version) >= 26;
}
