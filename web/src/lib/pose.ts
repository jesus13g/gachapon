/** Pose of the WebGL capsule in viewBox units. Pure data: lives outside the (lazy) three.js chunk. */
export interface Pose {
  vx: number;
  vy: number;
  /** capsule length */
  size: number;
  rx: number;
  ry: number;
  rz: number;
  /** 0 closed → 1 fully open */
  open: number;
  /** canvas opacity (0 = not rendered at all) */
  a: number;
  /** amount of idle float / pointer reaction */
  idle: number;
  color: number;
  /** intro-only factors (kept apart so the load animation never fights the scroll timeline) */
  boot: number;
  pop: number;
}
export const makePose = (): Pose => ({ vx: 1130, vy: 330, size: 190, rx: 0.25, ry: -0.5, rz: -0.42, open: 0, a: 0, idle: 1, color: 0xff9a3c, boot: 1, pop: 1 });
