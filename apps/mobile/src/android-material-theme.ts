import schemes from "./android-material-schemes.json";
import type { ThemeId } from "./app-theme";

// Precomputed with Google's @material/material-color-utilities 0.3.0,
// themeFromSourceColor. Neutral tones: light94/dark12 surface container.
// No runtime color analysis, downloads or generated theme files.
export function androidMaterialTheme(id: ThemeId, dark: boolean) {
  return schemes[id][dark ? "dark" : "light"];
}
