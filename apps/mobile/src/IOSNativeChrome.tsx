import React from "react";
import { Platform, type NativeSyntheticEvent, type StyleProp, type ViewStyle } from "react-native";
import { requireNativeView } from "expo";

// The workspace also contains desktop React 19 types. Keep this native view
// adapter typed against the mobile React 18 surface (Metro pins its runtime).
const nativeView = requireNativeView as unknown as <P>(name: string) => React.ComponentType<P>;

interface ButtonProps {
  title?: string; subtitle?: string; row?: boolean; selected?: boolean; choices?: string[]; choiceIndex?: number; onChoice?(event: NativeSyntheticEvent<{ index: number }>): void;
  symbol: string; label: string; enabled: boolean; prominent: boolean; symbolSize: number;
  style: StyleProp<ViewStyle>; onPress(): void;
}
interface TabsProps { selected: number; style: StyleProp<ViewStyle>; onChange(event: NativeSyntheticEvent<{ index: number }>): void }
interface SliderProps { value: number; style: StyleProp<ViewStyle>; onChange(event: NativeSyntheticEvent<{ value: number }>): void; onTracking(event: NativeSyntheticEvent<{ active: boolean }>): void }
interface StepperProps { value: number; enabled: boolean; style: StyleProp<ViewStyle>; onChange(event: NativeSyntheticEvent<{ value: number }>): void }
interface SurfaceProps { style: StyleProp<ViewStyle>; pointerEvents: "none" }

// Do not request iOS-only managers on Android or in an old installed native
// binary/Expo Go. The existing React Native controls remain the fallback.
let Button: React.ComponentType<ButtonProps> | null = null;
let Tabs: React.ComponentType<TabsProps> | null = null;
let Slider: React.ComponentType<SliderProps> | null = null;
let Stepper: React.ComponentType<StepperProps> | null = null;
let Surface: React.ComponentType<SurfaceProps> | null = null;
if (Platform.OS === "ios" && (globalThis as any).expo?.modules?.SdaGlassButton) {
  try {
    Button = nativeView<ButtonProps>("SdaGlassButton");
    Tabs = nativeView<TabsProps>("SdaGlassTabs");
    Slider = nativeView<SliderProps>("SdaNativeSlider");
    Stepper = nativeView<StepperProps>("SdaNativeStepper");
    Surface = nativeView<SurfaceProps>("SdaMaterialSurface");
  } catch { /* Older native binary: retain the previous UI. */ }
}
export const hasNativeIOSChrome = Button !== null && Tabs !== null && Surface !== null && Slider !== null && Stepper !== null;

export function IOSIconButton({ symbol, label, onPress, disabled = false, prominent = false, size = 46, symbolSize = 20 }: {
  symbol: string; label: string; onPress(): void; disabled?: boolean; prominent?: boolean; size?: number; symbolSize?: number;
}) {
  if (!Button) return null;
  return <Button symbol={symbol} label={label} enabled={!disabled} prominent={prominent} symbolSize={symbolSize} onPress={onPress} style={{ width: size, height: size }} />;
}
export function IOSGlassTabs({ selected, onChange }: { selected: number; onChange(index: number): void }) {
  if (!Tabs) return null;
  return <Tabs selected={selected} onChange={event => onChange(event.nativeEvent.index)} style={{ width: "100%", maxWidth: 390, height: 66, alignSelf: "center" }} />;
}
export function IOSMaterialSurface({ style }: { style: StyleProp<ViewStyle> }) {
  if (!Surface) return null;
  return <Surface pointerEvents="none" style={style} />;
}

export function IOSAction({ symbol, title, subtitle = "", label = title, onPress, disabled = false, row = false, selected = false, style, choices = [], choiceIndex = -1, onChoice }: {
  symbol: string; title: string; subtitle?: string; label?: string; onPress(): void; disabled?: boolean; row?: boolean; selected?: boolean; style?: StyleProp<ViewStyle>; choices?: string[]; choiceIndex?: number; onChoice?(index: number): void;
}) {
  if (!Button) return null;
  return <Button symbol={symbol} title={title} subtitle={subtitle} label={label} row={row} selected={selected} enabled={!disabled} prominent={false} symbolSize={18} choices={choices} choiceIndex={choiceIndex} onChoice={event => onChoice?.(event.nativeEvent.index)} onPress={onPress} style={[{ width: "100%", height: subtitle ? 76 : 50 }, style]} />;
}
export function IOSVolumeSlider({ value, onChange, onTracking }: { value: number; onChange(value: number): void; onTracking(active: boolean): void }) {
  if (!Slider) return null;
  return <Slider value={value} onChange={event => onChange(event.nativeEvent.value)} onTracking={event => onTracking(event.nativeEvent.active)} style={{ flex: 1, height: 44 }} />;
}
export function IOSDistanceStepper({ value, disabled, onChange }: { value: number; disabled: boolean; onChange(value: number): void }) {
  if (!Stepper) return null;
  return <Stepper value={value} enabled={!disabled} onChange={event => onChange(event.nativeEvent.value)} style={{ width: 94, height: 44 }} />;
}
