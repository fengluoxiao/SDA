import { useEffect, useRef, useState } from "react";
import { Platform } from "react-native";
import { CardboardPose, cardboardIOSScreenAngle } from "./cardboard-pose";
import { acquireCardboardOrientation } from "./cardboard-session";

let sessionSerial = 0;

/** Requested view survives backgrounding; only foreground motion/awake work pauses. */
export function useCardboardSession(requested: boolean, frameRate: number, foreground = true) {
  const pose = useRef(new CardboardPose());
  const screenAngle = useRef<number | undefined>(undefined);
  const [status, setStatus] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    pose.current.reset(); screenAngle.current = undefined; setStatus("");
    if (!requested || Platform.OS === "web") return;
    let active = true, orientationRevision = 0, release: (() => void) | undefined;
    let subscription: { remove(): void } | undefined;
    try {
      const orientation: typeof import("expo-screen-orientation") = require("expo-screen-orientation");
      release = acquireCardboardOrientation(orientation, orientation.OrientationLock.LANDSCAPE, () => { if (active) setStatus("请手动把手机转成横屏"); });
      if (Platform.OS === "ios") {
        const apply = (value: number) => { const angle = cardboardIOSScreenAngle(value); if (active && angle !== undefined) screenAngle.current = angle; };
        subscription = orientation.addOrientationChangeListener(event => { ++orientationRevision; apply(event.orientationInfo.orientation); });
        const snapshot = orientationRevision;
        void orientation.getOrientationAsync().then(value => { if (snapshot === orientationRevision) apply(value); }).catch(() => {});
      }
    } catch { setStatus("横屏模块不可用，请手动横屏或更新安装包"); }
    return () => { active = false; subscription?.remove(); release?.(); pose.current.reset(); screenAngle.current = undefined; };
  }, [requested]);
  useEffect(() => {
    if (!requested || !foreground) return;
    let active = true, subscription: { remove(): void } | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let latestSample = 0;
    let awake: typeof import("expo-keep-awake") | undefined;
    const tag = "sda-cardboard-vr-" + (++sessionSerial);
    let currentStatus = "";
    const fail = (message: string) => { if (active && message !== currentStatus) { currentStatus = message; setStatus(message); } };
    const start = async () => {
      if (Platform.OS === "web") { fail("浏览器预览不接手机姿态；请用 Expo Go 或安装包体验"); return; }
      try {
        awake = require("expo-keep-awake");
        await awake!.activateKeepAwakeAsync(tag).catch(() => {});
        if (!active) { void awake!.deactivateKeepAwake(tag).catch(() => {}); return; }
        const motion: typeof import("expo-sensors").DeviceMotion = require("expo-sensors").DeviceMotion;
        if (!await motion.isAvailableAsync()) { fail("设备不支持手机姿态传感器"); return; }
        if (!active) return;
        const permission = await motion.requestPermissionsAsync();
        if (!active) return;
        if (permission.status !== "granted") { fail("请在系统设置中允许 SDA 访问运动与健身，再点重试"); return; }
        motion.setUpdateInterval(frameRate >= 120 ? 8 : 16);
        fail("正在连接手机姿态…");
        subscription = motion.addListener(event => {
          if (!active || !event.rotation) return;
          const angle = Platform.OS === "ios" ? screenAngle.current ?? event.orientation * Math.PI / 180 : event.orientation * Math.PI / 180;
          if (pose.current.update(event.rotation, angle, performance.now(), Platform.OS === "ios" ? "ios" : "android")) {
            latestSample = performance.now(); fail("");
          }
        });
        timeout = setInterval(() => { if (!latestSample || performance.now() - latestSample > 2000) fail("暂未收到手机姿态，请点重试"); }, 3000);
      } catch { fail("手机姿态模块不可用，请更新安装包或 Expo Go 后重试"); }
    };
    void start();
    return () => {
      active = false; subscription?.remove(); if (timeout) clearInterval(timeout);
      void awake?.deactivateKeepAwake(tag).catch(() => {});
    };
  }, [requested, foreground, frameRate, revision]);
  return { pose, status, retry: () => setRevision(value => value + 1), recenter: () => pose.current.recenter() };
}
