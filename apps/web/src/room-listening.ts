// Play the room response as authored. Do not add default reflection attenuation
// or loudness compensation; intentional calibration remains a separate control.
export const ROOM_LISTENING_LEVELS = {directDb:0,earlyDb:0,lateDb:0,earlyMs:50} as const;
