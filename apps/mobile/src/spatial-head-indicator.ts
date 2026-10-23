// One visual state for the native menus and React Native fallbacks.
export function spatialHeadIndicator(spatialEnabled: boolean, trackingEnabled: boolean, headYaw: number, accent: string, inactive: string) {
  const tracking = spatialEnabled && trackingEnabled;
  const yaw = Number.isFinite(headYaw) ? ((headYaw + 180) % 360 + 360) % 360 - 180 : 0;
  return {
    active: spatialEnabled, tracking,
    color: spatialEnabled ? accent : inactive,
    symbol: tracking ? "person.crop.circle.fill" as const : "person.crop.circle" as const,
    poseDegrees: tracking ? Math.max(-45, Math.min(45, yaw)) : 0,
    label: !spatialEnabled ? "空间音频 · 关闭" : tracking ? "空间音频 · 头部追踪" : "空间音频 · 固定",
  };
}
