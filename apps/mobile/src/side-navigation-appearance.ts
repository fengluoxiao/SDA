// The custom vertical rail does not receive UIKit tab-bar tint adaptation.
// Approximate the approved native selection using a darker, same-hue accent
// only in light mode over a dark surface. Light library pages retain the
// original accent and subtle selection, as does the native portrait bar.
export function sideNavigationAppearance(accent: string, dark: boolean, backgroundColor = "#ffffff") {
  // ArtworkColor and the GL sampler return rgb(...); page palettes use hex.
  // Both must classify identically or sampled playback backgrounds silently
  // fall back to the light-page selection.
  const surface = backgroundColor.trim();
  const rgb = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/i.exec(surface);
  const background = /^#[0-9a-f]{6}$/i.test(surface)
    ? [1, 3, 5].map(index => parseInt(surface.slice(index, index + 2), 16))
    : rgb && rgb.slice(1).every(channel => Number(channel) <= 255) ? rgb.slice(1).map(Number) : null;
  const deepenSelection = !dark && background !== null &&
    (background[0]! * 0.299 + background[1]! * 0.587 + background[2]! * 0.114) < 128;
  let selectedIcon = accent;
  if (deepenSelection && /^#[0-9a-f]{6}$/i.test(accent)) {
    const rgb = [1, 3, 5].map(index => parseInt(accent.slice(index, index + 2), 16) / 255);
    const high = Math.max(...rgb), low = Math.min(...rgb);
    const lightness = (high + low) / 2;
    const chroma = high - low;
    const saturation = chroma === 0 ? 0 : chroma / (1 - Math.abs(2 * lightness - 1));
    const selectedLightness = lightness * 0.55;
    const selectedChroma = (1 - Math.abs(2 * selectedLightness - 1)) * saturation;
    const offset = selectedLightness - selectedChroma / 2;
    selectedIcon = "#" + rgb.map(channel => {
      const value = chroma === 0 ? selectedLightness : (channel - low) / chroma * selectedChroma + offset;
      return Math.round(Math.min(1, Math.max(0, value)) * 255).toString(16).padStart(2, "0");
    }).join("");
  }
  return {
    selectedIcon,
    inactiveIcon: dark ? "#FFFFFF" : "#000000",
    selectionFill: dark ? "rgba(255,255,255,0.14)" : deepenSelection ? "rgba(0,0,0,0.34)" : "rgba(118,118,128,0.18)",
  };
}
