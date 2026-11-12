// Generated from Folia GPL-3.0 source; see upstream.json and LICENSE. Do not edit function bodies.
type Theme = { backgroundColor: string; primaryColor: string; secondaryColor: string; accentColor: string };
type LatentBackgroundColorSource = "cover-only" | "cover-theme";
export const DEFAULT_LATENT_BACKGROUND_TUNING = {
  displayMode: 'both',
  colorSource: 'cover-theme',
  dynamicOnlyInPlayer: true,
  enhancedBeatResponse: true,
  ditheringSpeed: 0.1,
  ditheringAudioSpeed: 1.2,
  ditheringSize: 2.5,
  ditheringOpacity: 0.55,
  meshSpeed: 0.3,
  meshAudioSpeed: 2,
  meshDistortion: 0.8,
  meshSwirl: 0.1,
  overlayEnabled: true,
  overlayOpacity: 0.35,
} as const;
export const MAX_SHADER_PIXELS = 1280 * 720;
export const PAUSED_SPEED_SCALE = 0.12;
export const PAUSE_EASE_MS = 300;
export const normalizeAudio = (value: number) => Math.min(1, Math.max(0, value / 255));
export const clampShaderSpeed = (value: number) => Math.min(2, Math.max(0, value));
export const clampAudioAmount = (value: number) => Math.min(1, Math.max(0, value));
export const easeTowards = (current: number, target: number, amount: number) => (
    current + (target - current) * amount
);
export const resolveLatentBroadbandEnergy = (
    bass: number,
    lowMid: number,
    mid: number,
    vocal: number,
    treble: number,
) => {
    const broadbandEnergy = (
        normalizeAudio(bass) * 0.22
        + normalizeAudio(lowMid) * 0.18
        + normalizeAudio(mid) * 0.22
        + normalizeAudio(vocal) * 0.28
        + normalizeAudio(treble) * 0.1
    );
    return Math.pow(broadbandEnergy, 0.55);
};
export const resolveLatentOnsetPulse = (
    currentEnergy: number,
    previousEnergy: number,
    previousPulse: number,
) => Math.max(
    previousPulse * 0.84,
    clampAudioAmount((currentEnergy - previousEnergy) * 7),
);
export const resolveLatentBeatSpeedTarget = (
    broadbandEnergy: number,
    onsetPulse: number,
) => clampAudioAmount(broadbandEnergy * 0.42 + onsetPulse * 0.85);
export const resolveLatentBeatFxPulse = (beat: number, downbeat: number) => Math.max(beat, downbeat);
export const resolveLatentAudioSpeedTarget = (
    broadbandEnergy: number,
    onsetPulse: number,
    enhancedBeatResponse: boolean,
) => enhancedBeatResponse
    ? resolveLatentBeatSpeedTarget(broadbandEnergy, onsetPulse)
    : clampAudioAmount(broadbandEnergy);
export const resolveLatentShaderSpeed = (
    baseSpeed: number,
    audioSpeed: number,
    audioAmount: number,
    paused: boolean,
) => clampShaderSpeed(
    paused
        ? baseSpeed * PAUSED_SPEED_SCALE
        : easeTowards(baseSpeed, audioSpeed, audioAmount),
);
export const resolveLatentShaderColors = (
    coverColors: string[],
    theme: Theme,
    colorSource: LatentBackgroundColorSource,
) => {
    const primary = coverColors[0] ?? theme.secondaryColor;
    if (colorSource === 'cover-only') {
        const secondary = coverColors[1] ?? primary;
        const tertiary = coverColors[2] ?? secondary;
        const quaternary = coverColors[3] ?? primary;
        const quinary = coverColors[4] ?? secondary;
        const senary = coverColors[5] ?? tertiary;
        return {
            ditheringBack: tertiary,
            ditheringFront: primary,
            mesh: [primary, secondary, tertiary, quaternary, quinary, senary],
        };
    }
    const secondary = coverColors[1] ?? theme.primaryColor;
    const tertiary = coverColors[2] ?? primary;
    const quaternary = coverColors[3] ?? secondary;
    return {
        ditheringBack: theme.backgroundColor,
        ditheringFront: primary,
        mesh: [primary, secondary, tertiary, quaternary, theme.backgroundColor, theme.accentColor],
    };
};
