import { gsap } from 'gsap';
import { MotionPathPlugin } from 'gsap/MotionPathPlugin';
import type { Ctx } from '../lib/motion';
import type { Pose } from '../lib/capsule3d';
import { STORY } from '../lib/tokens';
import * as A from './acts-a';
import * as B from './acts-b';
import * as C from './acts-c';
import * as D from './acts-d';
import * as E from './acts-e';

gsap.registerPlugin(MotionPathPlugin);
void STORY;

export type Env = { pose: Pose; gl: boolean };
export type ActBuilder = (c: Ctx, env: Env) => void;

export const BUILDERS: Record<string, ActBuilder> = {
  boot: A.boot,
  problem: A.problem,
  roles: A.roles,
  bus: B.bus,
  locks: B.locks,
  modes: B.modes,
  control: C.control,
  teams: C.teams,
  routines: C.routines,
  knowledge: D.knowledge,
  mcp: D.mcp,
  sandbox: D.sandbox,
  journal: E.journal,
  interfaces: E.interfaces,
  system: E.system,
  final: E.final,
};
