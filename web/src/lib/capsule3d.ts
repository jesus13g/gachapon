import * as THREE from 'three';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';
import { VB } from './tokens';
import type { Pose } from './pose';

/**
 * The only WebGL piece of the page: the capsule seen up close (hero and final act).
 * Everything else is SVG/HTML. One canvas, one object, rendered only while it is visible.
 * Its screen position is expressed in the same viewBox units as the SVG scenes, so it hands over to the
 * 2D story capsule without a seam.
 */
export type { Pose };

const R = 0.22; // radius; total length = 1
const HALF_CYL = 0.5 - R;

function halfCapsule(color: THREE.ColorRepresentation, side: 1 | -1) {
  const pts: THREE.Vector2[] = [new THREE.Vector2(0, 0)];
  pts.push(new THREE.Vector2(R, 0));
  pts.push(new THREE.Vector2(R, HALF_CYL));
  for (let i = 1; i <= 14; i++) {
    const a = (i / 14) * (Math.PI / 2);
    pts.push(new THREE.Vector2(Math.cos(a) * R, HALF_CYL + Math.sin(a) * R));
  }
  const g = new THREE.LatheGeometry(pts, 48);
  g.rotateZ(side === 1 ? -Math.PI / 2 : Math.PI / 2);
  const m = new THREE.MeshPhysicalMaterial({ color, roughness: 0.28, metalness: 0.05, clearcoat: 1, clearcoatRoughness: 0.12 });
  return new THREE.Mesh(g, m);
}

export interface Capsule3D {
  resize(): void;
  setPointer(nx: number, ny: number): void;
  dispose(): void;
}

export function createCapsule3D(canvas: HTMLCanvasElement, host: HTMLElement, pose: Pose, tick: (fn: () => void) => () => void): Capsule3D | null {
  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: 'high-performance' });
  } catch {
    return null;
  }
  if (!renderer.getContext()) return null;

  renderer.setClearColor(0x000000, 0);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.outputColorSpace = THREE.SRGBColorSpace;

  const scene = new THREE.Scene();
  const pmrem = new THREE.PMREMGenerator(renderer);
  const envRT = pmrem.fromScene(new RoomEnvironment(), 0.04);
  scene.environment = envRT.texture;
  scene.environmentIntensity = 0.9;

  const FOV = 28;
  const DIST = 10;
  const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 50);
  camera.position.set(0, 0, DIST);

  const group = new THREE.Group();
  const left = halfCapsule(pose.color, -1);
  const right = halfCapsule(0xf1f0ea, 1);
  const payload = new THREE.Mesh(
    new THREE.SphereGeometry(R * 0.8, 24, 16),
    new THREE.MeshBasicMaterial({ color: 0x3ddc84, transparent: true, opacity: 0 }),
  );
  const glow = new THREE.PointLight(0x3ddc84, 0, 4, 2);
  group.add(left, right, payload, glow);
  scene.add(group);

  const rim = new THREE.DirectionalLight(0xffb878, 2.2);
  rim.position.set(-3, 2, 3);
  scene.add(rim);

  let W = 1;
  let H = 1;
  let k = 1; // px per viewBox unit
  let ox = 0;
  let oy = 0;
  let worldPerPx = 1;
  const px = { x: 0, y: 0 };
  let lastA = -1;

  const resize = () => {
    const r = host.getBoundingClientRect();
    W = Math.max(1, Math.round(r.width));
    H = Math.max(1, Math.round(r.height));
    const dpr = Math.min(window.devicePixelRatio || 1, 1.75);
    renderer.setPixelRatio(dpr);
    renderer.setSize(W, H, false);
    camera.aspect = W / H;
    camera.updateProjectionMatrix();
    k = Math.min(W / VB.w, H / VB.h);
    ox = (W - VB.w * k) / 2;
    oy = (H - VB.h * k) / 2;
    worldPerPx = (2 * Math.tan((FOV * Math.PI) / 360) * DIST) / H;
  };
  resize();

  const t0 = performance.now();
  const stop = tick(() => {
    const alpha = pose.a * pose.boot;
    if (alpha !== lastA) {
      canvas.style.opacity = String(alpha);
      lastA = alpha;
    }
    if (alpha < 0.01) return;
    const t = (performance.now() - t0) / 1000;
    const idle = pose.idle;
    px.x = ox + pose.vx * k;
    px.y = oy + pose.vy * k + Math.sin(t * 1.3) * 6 * idle * k;
    group.position.set((px.x - W / 2) * worldPerPx, -(px.y - H / 2) * worldPerPx, 0);
    const L = pose.size * pose.pop * k * worldPerPx;
    group.scale.setScalar(L);
    group.rotation.set(pose.rx + ptr.y * 0.18 * idle + Math.sin(t * 0.9) * 0.04 * idle, pose.ry + ptr.x * 0.3 * idle, pose.rz + Math.sin(t * 0.7) * 0.03 * idle);
    const o = pose.open * 0.3;
    left.position.x = -o;
    right.position.x = o;
    (left.material as THREE.MeshPhysicalMaterial).color.setHex(pose.color);
    const pm = payload.material as THREE.MeshBasicMaterial;
    pm.opacity = Math.min(1, pose.open * 1.6);
    payload.scale.setScalar(0.3 + pose.open * 0.9);
    glow.intensity = pose.open * 3;
    renderer.render(scene, camera);
  });

  const ptr = { x: 0, y: 0 };
  const target = { x: 0, y: 0 };
  const ease = tick(() => {
    ptr.x += (target.x - ptr.x) * 0.08;
    ptr.y += (target.y - ptr.y) * 0.08;
  });

  return {
    resize,
    setPointer(nx, ny) {
      target.x = nx;
      target.y = ny;
    },
    dispose() {
      stop();
      ease();
      left.geometry.dispose();
      right.geometry.dispose();
      (left.material as THREE.Material).dispose();
      (right.material as THREE.Material).dispose();
      payload.geometry.dispose();
      (payload.material as THREE.Material).dispose();
      envRT.dispose();
      pmrem.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
    },
  };
}
