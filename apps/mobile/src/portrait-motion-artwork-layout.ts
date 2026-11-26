export function portraitMotionArtworkLayout(width: number, height: number, sourceAspectRatio: number) {
  const aspect = Number.isFinite(sourceAspectRatio) && sourceAspectRatio > 0 ? Math.max(.25, Math.min(4, sourceAspectRatio)) : .75;
  const imageHeight = Math.max(0, width) / aspect;
  return {
    frame: { left: 0, top: 0, width: Math.max(0, width), height: imageHeight },
    featherHeight: Math.min(imageHeight, Math.max(0, height) * .60),
  };
}
