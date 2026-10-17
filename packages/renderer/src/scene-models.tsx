import React, { useEffect, useMemo } from "react";
import * as THREE from "three";
// This leaf component is platform-neutral (no DOM or web Canvas imports).
import { RoundedBox } from "@react-three/drei/core/RoundedBox";
import { SCENE_ROOM_HALF_EXTENT as ROOM, SCENE_FLOOR_Y as FLOOR_Y, SCENE_WALL_HEIGHT as WALL_H, SCENE_WALL_MID_Y as WALL_MID_Y } from "./scene-coordinates";
type Theme = "dark" | "light";

export const PALETTE = {
  dark: {
    bg: "#171819",
    floor: "#242827",
    gridMain: "#4c5851",
    gridWall: "#333b36",
    outline: "#637168",
    floorGrid: "#343d37",
  },
  light: {
    bg: "#eceeed",
    floor: "#dce2dd",
    gridMain: "#9aab9f",
    gridWall: "#b8c5bc",
    outline: "#879c8e",
    floorGrid: "#b9c8be",
  },
} as const;

export type Palette = (typeof PALETTE)[Theme];


export function GenelecSpeaker() {
  return (
    <group>
      {/* 圆角箱体（Polar White 极地白） */}
      <RoundedBox userData={{ immersiveCabinet: true }} args={[0.13, 0.18, 0.11]} radius={0.03} smoothness={2}>
        <meshStandardMaterial color="#e8eaec" roughness={0.4} metalness={0.15} />
      </RoundedBox>
      {/* 正面大椭圆波导（DCW，覆盖整个前障板） */}
      <mesh position={[0, 0, 0.05]} scale={[1, 1.35, 0.35]}>
        <sphereGeometry args={[0.052, 12, 12]} />
        <meshStandardMaterial color="#c9ced4" roughness={0.45} metalness={0.1} />
      </mesh>
      {/* 中央同轴中高音单元 */}
      <mesh position={[0, 0, 0.062]} scale={[1, 1, 0.5]}>
        <sphereGeometry args={[0.016, 10, 10]} />
        <meshStandardMaterial color="#1c2026" roughness={0.3} metalness={0.5} />
      </mesh>
      {/* Iso-Pod 避震支架（微后倾） */}
      <mesh position={[0, -0.1, 0]} rotation={[0.12, 0, 0]}>
        <boxGeometry args={[0.07, 0.018, 0.08]} />
        <meshStandardMaterial color="#aab0b8" roughness={0.55} />
      </mesh>
    </group>
  );
}

/** 真力 7350A 低音炮：矮胖的圆角箱体 + 正面低音单元 + 支脚，放地板上。 */
export function GenelecSub() {
  return (
    <group>
      <RoundedBox userData={{ immersiveCabinet: true }} args={[0.24, 0.22, 0.2]} radius={0.04} smoothness={2}>
        <meshStandardMaterial color="#e8eaec" roughness={0.4} metalness={0.15} />
      </RoundedBox>
      {/* 正面低音单元（纸盆）—— 前移避开与箱体面板的 z-fighting */}
      <mesh position={[0, -0.02, 0.106]} rotation={[Math.PI / 2, 0, 0]}>
        <cylinderGeometry args={[0.07, 0.07, 0.02, 16]} />
        <meshStandardMaterial color="#1c2026" roughness={0.35} metalness={0.4} />
      </mesh>
      {/* 防尘帽 */}
      <mesh position={[0, -0.02, 0.124]} scale={[1, 1, 0.5]}>
        <sphereGeometry args={[0.028, 10, 10]} />
        <meshStandardMaterial color="#2b2f35" roughness={0.4} />
      </mesh>
      {/* 四只支脚 */}
      {([[-0.08, -0.06], [0.08, -0.06], [-0.08, 0.06], [0.08, 0.06]] as const).map(([fx, fz], i) => (
        <mesh key={i} position={[fx, -0.115, fz]}>
          <cylinderGeometry args={[0.015, 0.018, 0.02, 12]} />
          <meshStandardMaterial color="#aab0b8" roughness={0.55} />
        </mesh>
      ))}
    </group>
  );
}


export function Room({ p }: { p: Palette }) {
  const geometry = useMemo(() => new THREE.BoxGeometry(ROOM * 2, WALL_H, ROOM * 2), []);
  return (
    <group>
      {/* 地板（听者脚下）：半透明且不写深度 —— ADM z 为负的对象（测试文件
          常有固定环绕一圈的低空对象）位于地板以下，实体地板会把它们整个
          遮住，必须能透过去看到 */}
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, FLOOR_Y + 0.001, 0]}>
        <planeGeometry args={[ROOM * 2, ROOM * 2]} />
        <meshBasicMaterial color={p.floor} transparent opacity={0.45} depthWrite={false} />
      </mesh>
      {/* 墙面参考网格（4 × WALL_H，压扁局部高度轴） */}
      <gridHelper
        args={[ROOM * 2, 10, p.gridMain, p.gridWall]}
        rotation={[Math.PI / 2, 0, 0]}
        position={[0, WALL_MID_Y, -ROOM + 0.002]}
        scale={[1, 1, WALL_H / (ROOM * 2)]}
      />
      <gridHelper
        args={[ROOM * 2, 10, p.gridMain, p.gridWall]}
        rotation={[0, 0, Math.PI / 2]}
        position={[-ROOM + 0.002, WALL_MID_Y, 0]}
        scale={[WALL_H / (ROOM * 2), 1, 1]}
      />
      {/* 房间轮廓线（地板到天花板的矩形房间） */}
      <lineSegments position={[0, WALL_MID_Y, 0]}>
        <edgesGeometry args={[geometry]} />
        <lineBasicMaterial color={p.outline} />
      </lineSegments>
    </group>
  );
}


export function Listener() {
  const gray = "#a3a3a3";
  const grayDark = "#898989";
  return (
    <group
      name="listener"
      position={[0, 0, 0]}
      onClick={(event) => event.stopPropagation()}
      onPointerOver={(event) => event.stopPropagation()}
      onPointerMove={(event) => event.stopPropagation()}
    >
      {/* 头：蛋形（略高、前后稍扁） */}
      <mesh scale={[0.85, 1.08, 0.92]}>
        <sphereGeometry args={[0.16, 16, 16]} />
        <meshStandardMaterial color={gray} roughness={0.5} />
      </mesh>
      {/* 面部（-z = 前方）：闭眼眼睑、鼻梁、嘴 —— KEMAR 式浮雕感 */}
      {/* 眼睑：略暗的扁椭球，半嵌入表面 */}
      <mesh position={[-0.05, 0.04, -0.128]} scale={[1.1, 0.55, 0.5]}>
        <sphereGeometry args={[0.021, 10, 10]} />
        <meshStandardMaterial color={grayDark} roughness={0.55} />
      </mesh>
      <mesh position={[0.05, 0.04, -0.128]} scale={[1.1, 0.55, 0.5]}>
        <sphereGeometry args={[0.021, 10, 10]} />
        <meshStandardMaterial color={grayDark} roughness={0.55} />
      </mesh>
      {/* 鼻梁：竖向小椭球 */}
      <mesh position={[0, -0.012, -0.148]} scale={[0.5, 1, 0.75]}>
        <sphereGeometry args={[0.032, 10, 10]} />
        <meshStandardMaterial color={gray} roughness={0.5} />
      </mesh>
      {/* 嘴：细横条微凸 */}
      <mesh position={[0, -0.075, -0.132]}>
        <boxGeometry args={[0.052, 0.009, 0.012]} />
        <meshStandardMaterial color={grayDark} roughness={0.55} />
      </mesh>
      {/* 耳廓：两侧凸起，略靠后（+z 是后方），是前后朝向的提示 */}
      {[-1, 1].map((side) => (
        <group key={side} position={[side * 0.145, 0, 0.02]}>
          <mesh scale={[0.28, 0.75, 0.55]}>
            <sphereGeometry args={[0.06, 12, 12]} />
            <meshStandardMaterial color={grayDark} roughness={0.55} />
          </mesh>
          <mesh position={[side * 0.014, 0, -0.004]} scale={[0.16, 0.42, 0.3]}>
            <sphereGeometry args={[0.045, 10, 10]} />
            <meshStandardMaterial color={gray} roughness={0.5} />
          </mesh>
        </group>
      ))}
      {/* 颈部（平直切口的圆柱） */}
      <mesh position={[0, -0.2, 0]}>
        <cylinderGeometry args={[0.075, 0.08, 0.09, 24]} />
        <meshStandardMaterial color={grayDark} roughness={0.55} />
      </mesh>
      {/* 立杆 + 落地脚盘 */}
      <mesh position={[0, -0.43, 0]}>
        <cylinderGeometry args={[0.014, 0.014, 0.37, 12]} />
        <meshStandardMaterial color="#484848" roughness={0.4} metalness={0.4} />
      </mesh>
      <mesh position={[0, FLOOR_Y + 0.016, 0]}>
        <cylinderGeometry args={[0.09, 0.11, 0.024, 32]} />
        <meshStandardMaterial color="#484848" roughness={0.5} metalness={0.3} />
      </mesh>
    </group>
  );
}

/** MPEG-H OAM uses full-sphere directions, including negative elevation. */
export function SphericalRoom({ p }: { p: Palette }) {
  const geometry = useMemo(() => {
    const points: number[] = [];
    const segment = (a: number[], b: number[]) => points.push(...a, ...b);
    for (const elevation of [-60, -30, 0, 30, 60]) {
      const angle = elevation * Math.PI / 180;
      const radius = ROOM * Math.cos(angle), y = ROOM * Math.sin(angle);
      for (let i = 0; i < 96; i++) {
        const a = i * Math.PI / 48, b = (i + 1) * Math.PI / 48;
        segment([radius * Math.cos(a), y, radius * Math.sin(a)], [radius * Math.cos(b), y, radius * Math.sin(b)]);
      }
    }
    for (let meridian = 0; meridian < 6; meridian++) {
      const az = meridian * Math.PI / 6;
      for (let i = 0; i < 96; i++) {
        const a = i * Math.PI / 48, b = (i + 1) * Math.PI / 48;
        segment([ROOM * Math.cos(a) * Math.cos(az), ROOM * Math.sin(a), ROOM * Math.cos(a) * Math.sin(az)],
          [ROOM * Math.cos(b) * Math.cos(az), ROOM * Math.sin(b), ROOM * Math.cos(b) * Math.sin(az)]);
      }
    }
    return new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute(points, 3));
  }, []);
  useEffect(() => () => geometry.dispose(), [geometry]);
  return <group name="mpegh-spherical-field">
    <lineSegments geometry={geometry}><lineBasicMaterial color={p.outline} transparent opacity={0.55} depthWrite={false}/></lineSegments>
  </group>;
}
