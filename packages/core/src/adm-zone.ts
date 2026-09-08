/** ADM exclusion regions are expressed in nominal speaker coordinates. */
export type AdmZone =
  | { type: "cartesian"; min: [number, number, number]; max: [number, number, number] }
  | { type: "polar"; min: [number, number]; max: [number, number] };
