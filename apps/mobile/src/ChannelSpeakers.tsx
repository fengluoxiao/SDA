import React, { useMemo } from "react";
import { Object3D } from "three";
import { LAYOUTS } from "../../../packages/renderer/src/layouts";
import { SCENE_FLOOR_Y, speakerScenePosition } from "../../../packages/renderer/src/scene-coordinates";
import { GenelecSpeaker, GenelecSub } from "../../../packages/renderer/src/scene-models";

/** Visual channel references only. Object activity and audio stay independent. */
export function ChannelSpeakers({ layout, visible = false }: { layout: "2.0" | "7.1.4" | "360RA-13"; visible?: boolean }) {
  const placements = useMemo(() => LAYOUTS[layout].map(speaker => {
    const position = speakerScenePosition(speaker), facing = new Object3D();
    facing.position.set(...position); facing.lookAt(0, speaker.isLfe ? SCENE_FLOOR_Y + .13 : 0, 0);
    return { speaker, position, quaternion: facing.quaternion.toArray() as [number, number, number, number] };
  }), [layout]);
  if (!visible) return null;
  return <group name="channel-speaker-models">
    {placements.map(({ speaker, position, quaternion }) => <group key={speaker.name} name={speaker.name} position={position} quaternion={quaternion}>
      {speaker.isLfe ? <GenelecSub /> : <GenelecSpeaker />}
    </group>)}
  </group>;
}
