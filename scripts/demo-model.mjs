/**
 * Builds demo/models/sample.glb, the model the browser demo opens: a small
 * drone hangar on its apron, and a quadcopter flying a loop around it.
 *
 * Everything is generated here from three.js primitives — no third-party
 * asset, nothing to license — so the file can be rebuilt and changed at will:
 *
 *   node scripts/demo-model.mjs
 *
 * Metres, +Y up, the hangar's doors facing +Z, as glTF expects. Faceted
 * normals throughout, for the low-poly look; one animation clip, "Flight
 * loop", which loops seamlessly.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as THREE from "three";
import { GLTFExporter } from "three/examples/jsm/exporters/GLTFExporter.js";

// GLTFExporter assembles the binary through FileReader, which Node lacks.
globalThis.FileReader ??= class {
  readAsArrayBuffer(blob) {
    blob.arrayBuffer().then((buffer) => {
      this.result = buffer;
      this.onloadend?.();
    });
  }
  readAsDataURL(blob) {
    blob.arrayBuffer().then((buffer) => {
      this.result = `data:${blob.type || "application/octet-stream"};base64,${Buffer.from(buffer).toString("base64")}`;
      this.onloadend?.();
    });
  }
};

const root = fileURLToPath(new URL("..", import.meta.url));
const out = path.join(root, "demo", "models", "sample.glb");

// ---------------------------------------------------------------------------
// Materials: a small palette, physically based.
// ---------------------------------------------------------------------------

const mat = (name, color, { roughness = 0.7, metalness = 0, emissive, intensity = 1 } = {}) => {
  const material = new THREE.MeshStandardMaterial({ name, color, roughness, metalness });
  if (emissive !== undefined) {
    material.emissive.set(emissive);
    material.emissiveIntensity = intensity;
  }
  return material;
};

const M = {
  concrete: mat("Concrete", 0x737a82, { roughness: 0.9 }),
  concreteDark: mat("Concrete plinth", 0x5c636b, { roughness: 0.9 }),
  shell: mat("Hangar shell", 0xd7dde3, { roughness: 0.42, metalness: 0.55 }),
  rib: mat("Hangar ribs", 0x5d6773, { roughness: 0.38, metalness: 0.7 }),
  wall: mat("End walls", 0xc4ccd4, { roughness: 0.55, metalness: 0.35 }),
  door: mat("Doors", 0xe3a21a, { roughness: 0.5, metalness: 0.25 }),
  doorFrame: mat("Door frame", 0x3b4048, { roughness: 0.45, metalness: 0.6 }),
  paintYellow: mat("Paint yellow", 0xf2c230, { roughness: 0.6 }),
  paintWhite: mat("Paint white", 0xf1f3f5, { roughness: 0.6 }),
  padDark: mat("Pad asphalt", 0x2f3338, { roughness: 0.95 }),
  wood: mat("Crate wood", 0xa8743f, { roughness: 0.85 }),
  woodDark: mat("Crate slats", 0x7a5230, { roughness: 0.85 }),
  barrel: mat("Barrel paint", 0xb8392b, { roughness: 0.45, metalness: 0.45 }),
  barrelBand: mat("Barrel bands", 0x4a4f57, { roughness: 0.4, metalness: 0.7 }),
  cone: mat("Cone", 0xff6a1a, { roughness: 0.55 }),
  pole: mat("Pole", 0xb9c0c8, { roughness: 0.35, metalness: 0.8 }),
  sockOrange: mat("Windsock orange", 0xff5a1f, { roughness: 0.8 }),
  sockWhite: mat("Windsock white", 0xf3f3f3, { roughness: 0.8 }),
  bench: mat("Workbench", 0x46505c, { roughness: 0.5, metalness: 0.5 }),
  lightWarm: mat("Ceiling light", 0xffffff, { emissive: 0xffe2b0, intensity: 3 }),
  edgeBlue: mat("Edge lights", 0x1d4f91, { emissive: 0x3aa0ff, intensity: 4 }),
  beacon: mat("Beacon", 0x7a1010, { emissive: 0xff2a1f, intensity: 5 }),
  chargeRing: mat("Charging ring", 0x0f3d24, { emissive: 0x2bd67b, intensity: 3 }),
  // The drone.
  body: mat("Drone body", 0x2a2f37, { roughness: 0.38, metalness: 0.45 }),
  canopy: mat("Drone canopy", 0xeef1f4, { roughness: 0.28, metalness: 0.1 }),
  arm: mat("Drone arms", 0x3a414b, { roughness: 0.4, metalness: 0.5 }),
  motor: mat("Drone motors", 0x8a929c, { roughness: 0.25, metalness: 0.9 }),
  prop: mat("Propellers", 0x15171b, { roughness: 0.5 }),
  guard: mat("Prop guards", 0xc9d0d8, { roughness: 0.4, metalness: 0.2 }),
  ledGreen: mat("LED green", 0x0d4020, { emissive: 0x2bff7a, intensity: 6 }),
  ledRed: mat("LED red", 0x40100d, { emissive: 0xff3326, intensity: 6 }),
  lens: mat("Camera lens", 0x0b0c0e, { roughness: 0.1, metalness: 0.2 }),
};

// ---------------------------------------------------------------------------
// Geometry helpers: every part faceted.
// ---------------------------------------------------------------------------

/**
 * One normal per face: the low-poly look, whatever the primitive. A box
 * already has one, and keeps its index. Nothing is textured, so the UVs go:
 * they would be a third of the file.
 */
function faceted(geometry) {
  if (geometry.userData.faceted) return geometry;
  geometry.deleteAttribute("uv");
  const flat = geometry.type === "BoxGeometry" ? geometry : geometry.index ? geometry.toNonIndexed() : geometry;
  if (flat !== geometry) {
    flat.deleteAttribute("normal");
    flat.computeVertexNormals();
  }
  flat.userData.faceted = true;
  return flat;
}

/**
 * The same primitive asked for twice is one geometry, which the exporter
 * then writes once: twenty edge lights cost one box.
 */
const shapes = new Map();
const shared = (key, make) => {
  if (!shapes.has(key)) shapes.set(key, faceted(make()));
  return shapes.get(key);
};

function part(name, geometry, material, { at = [0, 0, 0], rot = [0, 0, 0], parent } = {}) {
  const mesh = new THREE.Mesh(faceted(geometry), material);
  mesh.name = name;
  mesh.position.set(...at);
  mesh.rotation.set(...rot);
  parent?.add(mesh);
  return mesh;
}

const box = (w, h, d) => shared(`box ${w} ${h} ${d}`, () => new THREE.BoxGeometry(w, h, d));
const cyl = (rt, rb, h, seg = 12) =>
  shared(`cyl ${rt} ${rb} ${h} ${seg}`, () => new THREE.CylinderGeometry(rt, rb, h, seg));

// ---------------------------------------------------------------------------
// The site.
// ---------------------------------------------------------------------------

const scene = new THREE.Scene();
scene.name = "Drone hangar";

const site = new THREE.Group();
site.name = "Site";
scene.add(site);

// The apron: a slab whose top is the ground, y = 0.
part("Apron", box(13, 0.24, 11.5), M.concrete, { at: [0, -0.12, 0.35], parent: site });

// Painted taxi line from the doors to the pad, and the apron's border.
part("Taxi line", box(0.12, 0.012, 0.95), M.paintYellow, { at: [0, 0.006, 1.6], parent: site });
for (const [x, z, w, d] of [
  [0, 5.8, 12.4, 0.1],
  [0, -5.1, 12.4, 0.1],
  [6.2, 0.35, 0.1, 10.9],
  [-6.2, 0.35, 0.1, 10.9],
]) {
  part("Apron border", box(w, 0.012, d), M.paintYellow, { at: [x, 0.006, z], parent: site });
}

// The landing pad: a yellow ring on asphalt, and the H.
const pad = new THREE.Group();
pad.name = "Landing pad";
pad.position.set(0, 0, 3.5);
site.add(pad);
part("Pad ring", cyl(1.6, 1.6, 0.02, 28), M.paintYellow, { at: [0, 0.01, 0], parent: pad });
part("Pad surface", cyl(1.42, 1.42, 0.03, 28), M.padDark, { at: [0, 0.015, 0], parent: pad });
part("H left", box(0.16, 0.012, 1.1), M.paintWhite, { at: [-0.38, 0.036, 0], parent: pad });
part("H right", box(0.16, 0.012, 1.1), M.paintWhite, { at: [0.38, 0.036, 0], parent: pad });
part("H bar", box(0.6, 0.012, 0.16), M.paintWhite, { at: [0, 0.036, 0], parent: pad });

// Blue edge lights around the apron.
for (let i = 0; i < 7; i++) {
  const x = -6 + i * 2;
  part("Edge light", box(0.14, 0.08, 0.14), M.edgeBlue, { at: [x, 0.04, 5.95], parent: site });
}
for (let i = 0; i < 6; i++) {
  const z = -4.6 + i * 2;
  part("Edge light", box(0.14, 0.08, 0.14), M.edgeBlue, { at: [6.35, 0.04, z], parent: site });
  part("Edge light", box(0.14, 0.08, 0.14), M.edgeBlue, { at: [-6.35, 0.04, z], parent: site });
}

// ---------------------------------------------------------------------------
// The hangar: a Quonset hut on a low plinth, doors slid open.
// ---------------------------------------------------------------------------

const R = 2.6;
const PLINTH = 0.4;
const LENGTH = 5.6;
const BACK = -4.6;
const FRONT = BACK + LENGTH;
const MID = (BACK + FRONT) / 2;
const SEGMENTS = 10;

const hangar = new THREE.Group();
hangar.name = "Hangar";
site.add(hangar);

// Arch: half a cylinder lying along Z. Turned about X, the cylinder's θ in
// [π/2, 3π/2] is the upper half.
const arch = new THREE.CylinderGeometry(R, R, LENGTH, SEGMENTS, 1, true, Math.PI / 2, Math.PI);
arch.rotateX(Math.PI / 2);
const shell = part("Shell", arch, M.shell, { at: [0, PLINTH, MID], parent: hangar });
shell.material.side = THREE.DoubleSide;

for (const side of [-1, 1]) {
  part("Plinth", box(0.24, PLINTH, LENGTH), M.concreteDark, {
    at: [side * (R - 0.1), PLINTH / 2, MID],
    parent: hangar,
  });
}

// Ribs every metre or so, standing just proud of the shell.
for (let i = 0; i <= 5; i++) {
  const z = BACK + 0.06 + (i * (LENGTH - 0.12)) / 5;
  part("Rib", new THREE.TorusGeometry(R + 0.035, 0.055, 4, SEGMENTS, Math.PI), M.rib, {
    at: [0, PLINTH, z],
    parent: hangar,
  });
}

/** The end wall's outline: the plinth's rectangle under the arch, `door` cut out of it. */
function endWall(door) {
  const shape = new THREE.Shape();
  shape.moveTo(-R, 0);
  if (door) {
    shape.lineTo(-door.w / 2, 0);
    shape.lineTo(-door.w / 2, door.h);
    shape.lineTo(door.w / 2, door.h);
    shape.lineTo(door.w / 2, 0);
  }
  shape.lineTo(R, 0);
  shape.lineTo(R, PLINTH);
  shape.absarc(0, PLINTH, R, 0, Math.PI, false);
  shape.lineTo(-R, 0);
  return new THREE.ExtrudeGeometry(shape, { depth: 0.1, bevelEnabled: false, curveSegments: SEGMENTS });
}

part("Back wall", endWall(null), M.wall, { at: [0, 0, BACK], parent: hangar });
const DOOR = { w: 2.7, h: 2.45 };
part("Front wall", endWall(DOOR), M.wall, { at: [0, 0, FRONT - 0.1], parent: hangar });

// Door frame and the sliding doors, open to either side.
part("Door header", box(DOOR.w + 0.3, 0.16, 0.16), M.doorFrame, {
  at: [0, DOOR.h + 0.08, FRONT + 0.06],
  parent: hangar,
});
for (const side of [-1, 1]) {
  part("Door jamb", box(0.14, DOOR.h, 0.16), M.doorFrame, {
    at: [side * (DOOR.w / 2 + 0.07), DOOR.h / 2, FRONT + 0.06],
    parent: hangar,
  });
  const door = new THREE.Group();
  door.name = side < 0 ? "Door left" : "Door right";
  door.position.set(side * (DOOR.w / 2 + 0.62), 0, FRONT + 0.2);
  hangar.add(door);
  part("Door panel", box(1.38, DOOR.h - 0.05, 0.07), M.door, { at: [0, (DOOR.h - 0.05) / 2, 0], parent: door });
  for (const y of [0.55, 1.25, 1.95]) {
    part("Door rail", box(1.38, 0.06, 0.03), M.doorFrame, { at: [0, y, 0.045], parent: door });
  }
}

// A beacon on a short mast at the front of the roof.
part("Mast", cyl(0.03, 0.03, 0.5, 6), M.pole, { at: [0, PLINTH + R + 0.22, FRONT - 0.4], parent: hangar });
part("Beacon", new THREE.IcosahedronGeometry(0.09, 0), M.beacon, {
  at: [0, PLINTH + R + 0.52, FRONT - 0.4],
  parent: hangar,
});

// Inside: a light strip under the apex, a workbench, the charging pad.
part("Ceiling light", box(0.16, 0.06, LENGTH - 1.2), M.lightWarm, {
  at: [0, PLINTH + R - 0.12, MID],
  parent: hangar,
});
part("Workbench", box(1.8, 0.85, 0.6), M.bench, { at: [-1.2, 0.425, BACK + 0.5], parent: hangar });
part("Bench top", box(1.9, 0.05, 0.66), M.wood, { at: [-1.2, 0.875, BACK + 0.5], parent: hangar });
part("Charging pad", cyl(0.75, 0.8, 0.06, 20), M.padDark, { at: [0.9, 0.03, MID - 0.4], parent: hangar });
part("Charging ring", new THREE.TorusGeometry(0.62, 0.03, 4, 24), M.chargeRing, {
  at: [0.9, 0.065, MID - 0.4],
  rot: [Math.PI / 2, 0, 0],
  parent: hangar,
});

// ---------------------------------------------------------------------------
// Props around the hangar.
// ---------------------------------------------------------------------------

function crate(name, size, at, turn = 0) {
  const group = new THREE.Group();
  group.name = name;
  group.position.set(...at);
  group.rotation.y = turn;
  site.add(group);
  part("Crate", box(size, size, size), M.wood, { at: [0, size / 2, 0], parent: group });
  // Slats on the four sides, standing out a little.
  for (const [x, z, ry] of [
    [0, size / 2 + 0.01, 0],
    [0, -size / 2 - 0.01, 0],
    [size / 2 + 0.01, 0, Math.PI / 2],
    [-size / 2 - 0.01, 0, Math.PI / 2],
  ]) {
    part("Slat", box(size * 0.95, 0.07, 0.025), M.woodDark, { at: [x, size * 0.5, z], rot: [0, ry, 0], parent: group });
    part("Slat", box(0.07, size * 0.95, 0.025), M.woodDark, {
      at: [x, size * 0.5, z],
      rot: [0, ry, Math.PI / 4],
      parent: group,
    });
  }
}
crate("Crate A", 0.7, [3.5, 0, -1.2], 0.12);
crate("Crate B", 0.7, [3.55, 0.7, -1.22], -0.18);
crate("Crate C", 0.5, [4.35, 0, -0.5], 0.6);

function barrel(at) {
  const group = new THREE.Group();
  group.name = "Barrel";
  group.position.set(...at);
  site.add(group);
  part("Barrel body", cyl(0.3, 0.3, 0.88, 12), M.barrel, { at: [0, 0.44, 0], parent: group });
  for (const y of [0.22, 0.66]) {
    part("Barrel band", cyl(0.312, 0.312, 0.05, 12), M.barrelBand, { at: [0, y, 0], parent: group });
  }
  part("Barrel lid", cyl(0.27, 0.27, 0.02, 12), M.barrelBand, { at: [0, 0.89, 0], parent: group });
}
barrel([-3.55, 0, -1.8]);
barrel([-3.6, 0, -1.05]);
barrel([-4.2, 0, -1.45]);

for (const [x, z] of [
  [-2.3, 3.5],
  [2.3, 3.5],
  [0, 5.35],
]) {
  const group = new THREE.Group();
  group.name = "Cone";
  group.position.set(x, 0, z);
  site.add(group);
  part("Cone base", box(0.34, 0.03, 0.34), M.cone, { at: [0, 0.015, 0], parent: group });
  part("Cone body", cyl(0.025, 0.14, 0.42, 10), M.cone, { at: [0, 0.24, 0], parent: group });
  part("Cone band", cyl(0.075, 0.097, 0.07, 10), M.paintWhite, { at: [0, 0.27, 0], parent: group });
}

// The windsock, catching an easterly.
const windsock = new THREE.Group();
windsock.name = "Windsock";
windsock.position.set(5.2, 0, 4.4);
site.add(windsock);
part("Windsock pole", cyl(0.04, 0.05, 3.2, 8), M.pole, { at: [0, 1.6, 0], parent: windsock });
const sock = new THREE.Group();
sock.name = "Sock";
sock.position.set(0, 3.1, 0);
sock.rotation.set(0, 0.35, -0.22);
windsock.add(sock);
const colors = [M.sockOrange, M.sockWhite, M.sockOrange, M.sockWhite, M.sockOrange];
for (let i = 0; i < 5; i++) {
  const r0 = 0.24 - i * 0.03;
  const r1 = r0 - 0.03;
  part("Sock band", cyl(r0, r1, 0.26, 10), colors[i], {
    at: [-(0.13 + i * 0.26), 0, 0],
    rot: [0, 0, Math.PI / 2],
    parent: sock,
  });
}

// ---------------------------------------------------------------------------
// The drone: a quadcopter about a metre across, front toward +Z.
// ---------------------------------------------------------------------------

// The airframe is built at a metre across and flown at one and a half, so
// it still reads from where the whole site is in view.
const drone = new THREE.Group();
drone.name = "Drone";
drone.scale.setScalar(1.5);
scene.add(drone);

part("Body", box(0.36, 0.12, 0.46), M.body, { parent: drone });
const dome = new THREE.SphereGeometry(0.2, 8, 4, 0, Math.PI * 2, 0, Math.PI / 2);
dome.scale(0.86, 0.55, 1.1);
part("Canopy", dome, M.canopy, { at: [0, 0.06, 0.02], parent: drone });
part("Skid left", box(0.03, 0.03, 0.42), M.arm, { at: [-0.13, -0.15, 0], parent: drone });
part("Skid right", box(0.03, 0.03, 0.42), M.arm, { at: [0.13, -0.15, 0], parent: drone });
for (const x of [-0.13, 0.13]) {
  part("Strut", box(0.025, 0.1, 0.025), M.arm, { at: [x, -0.1, 0.12], parent: drone });
  part("Strut", box(0.025, 0.1, 0.025), M.arm, { at: [x, -0.1, -0.12], parent: drone });
}
part("Gimbal", new THREE.IcosahedronGeometry(0.06, 0), M.body, { at: [0, -0.08, 0.2], parent: drone });
part("Lens", cyl(0.028, 0.028, 0.03, 8), M.lens, { at: [0, -0.08, 0.255], rot: [Math.PI / 2, 0, 0], parent: drone });

const ARM = 0.4;
const rotors = [];
const corners = [
  ["FL", 1, 1, M.ledGreen],
  ["FR", -1, 1, M.ledGreen],
  ["BL", 1, -1, M.ledRed],
  ["BR", -1, -1, M.ledRed],
];
for (const [label, sx, sz, led] of corners) {
  const x = sx * ARM;
  const z = sz * ARM;
  const length = Math.hypot(x, z);
  part(`Arm ${label}`, box(0.05, 0.04, length), M.arm, {
    at: [x / 2, 0.01, z / 2],
    rot: [0, Math.atan2(x, z), 0],
    parent: drone,
  });
  part(`Motor ${label}`, cyl(0.05, 0.055, 0.07, 10), M.motor, { at: [x, 0.045, z], parent: drone });
  part(`LED ${label}`, box(0.03, 0.025, 0.03), led, { at: [x * 0.78, -0.02, z * 0.78], parent: drone });
  part(`Guard ${label}`, new THREE.TorusGeometry(0.24, 0.012, 4, 20), M.guard, {
    at: [x, 0.09, z],
    rot: [Math.PI / 2, 0, 0],
    parent: drone,
  });
  const rotor = new THREE.Group();
  rotor.name = `Rotor ${label}`;
  rotor.position.set(x, 0.095, z);
  drone.add(rotor);
  part("Blade", box(0.42, 0.008, 0.045), M.prop, { rot: [0.12, 0, 0], parent: rotor });
  part("Hub", cyl(0.02, 0.02, 0.02, 8), M.motor, { parent: rotor });
  // Opposite corners turn the same way, as on a real quad.
  rotors.push({ rotor, spin: sx * sz });
}

// ---------------------------------------------------------------------------
// The flight: one loop round the hangar, eight seconds, seamless.
// ---------------------------------------------------------------------------

const DURATION = 8;
const STEPS = 96;
const LOOP_R = 4.5;
const LOOP_Z = -0.6;
const times = [];
const positions = [];
const rotations = [];
const euler = new THREE.Euler(0, 0, 0, "YXZ");
const q = new THREE.Quaternion();
for (let i = 0; i <= STEPS; i++) {
  const t = (i / STEPS) * DURATION;
  const phi = (i / STEPS) * Math.PI * 2;
  const x = LOOP_R * Math.sin(phi);
  const z = LOOP_Z + LOOP_R * Math.cos(phi);
  // Rising over the pad, dipping behind the hangar.
  const y = 4.1 + 0.4 * Math.cos(phi) + 0.12 * Math.sin(phi * 3);
  times.push(t);
  positions.push(x, y, z);
  // Nose along the path, tipped forward as a quad flies, banked into the
  // left-hand turn.
  const heading = Math.atan2(Math.cos(phi), -Math.sin(phi));
  euler.set(0.14, heading, -0.24);
  q.setFromEuler(euler);
  rotations.push(q.x, q.y, q.z, q.w);
}

const tracks = [
  new THREE.VectorKeyframeTrack("Drone.position", times, positions),
  new THREE.QuaternionKeyframeTrack("Drone.quaternion", times, rotations),
];

// Four turns a second; keys a third of a turn apart, which a slerp follows
// the right way round. 32 whole turns in the loop, so it closes up.
const TURNS = 4 * DURATION;
const SPIN_STEPS = TURNS * 3;
for (const { rotor, spin } of rotors) {
  const spinTimes = [];
  const values = [];
  for (let i = 0; i <= SPIN_STEPS; i++) {
    spinTimes.push((i / SPIN_STEPS) * DURATION);
    q.setFromAxisAngle(new THREE.Vector3(0, 1, 0), spin * ((i * Math.PI * 2) / 3));
    values.push(q.x, q.y, q.z, q.w);
  }
  tracks.push(new THREE.QuaternionKeyframeTrack(`${rotor.name}.quaternion`, spinTimes, values));
}

// Starts where the clip starts, for a viewer that does not play it.
drone.position.set(positions[0], positions[1], positions[2]);
drone.quaternion.set(rotations[0], rotations[1], rotations[2], rotations[3]);

const clip = new THREE.AnimationClip("Flight loop", DURATION, tracks);

// ---------------------------------------------------------------------------

scene.updateMatrixWorld(true);
const exporter = new GLTFExporter();
const glb = await exporter.parseAsync(scene, {
  binary: true,
  animations: [clip],
});
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, Buffer.from(glb));

let triangles = 0;
scene.traverse((object) => {
  if (!object.isMesh) return;
  const { index } = object.geometry;
  triangles += (index ? index.count : object.geometry.getAttribute("position").count) / 3;
});
console.log(`${path.relative(root, out)}: ${(glb.byteLength / 1024).toFixed(0)} kB, ${triangles} triangles`);
