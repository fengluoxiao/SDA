// A view-only native capability; checking it must not initialize a decoder.
export const supportsEmbeddedMotionArtwork=!!(globalThis as any).expo?.modules?.SdaMotionArtwork;
