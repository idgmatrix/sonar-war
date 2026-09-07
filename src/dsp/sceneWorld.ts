import { AcousticEntity } from '../core/entity/acousticEntity.ts';
import { World } from '../core/sim/world.ts';
import type { DemoScene } from './demoScenes.ts';
import { MERCHANT_PROFILE_CODES, MERCHANT_TONAL_OVERLAY_CODES } from './sourceProfiles.ts';

/** 정적 청감 비교 장면을 월드로 옮긴다. 속도는 순간 도플러/운항 상태이며 위치를 적분하지 않는다. */
export function worldFromScene(scene: DemoScene): World {
  const world = new World();
  const ownship = new AcousticEntity(world.ownshipId);
  world.add(ownship);
  const place = (entity: AcousticEntity, bearing: number, range: number, depth: number, radial: number, speed?: number) => {
    const az = bearing * Math.PI / 180;
    const dy = depth - ownship.position[1];
    const horizontal = Math.sqrt(Math.max(0, range * range - dy * dy));
    entity.position = [horizontal * Math.cos(az), depth, horizontal * Math.sin(az)];
    const direction = [entity.position[0] / range, dy / range, entity.position[2] / range];
    const tangent = Math.sqrt(Math.max(0, (speed ?? Math.abs(radial)) ** 2 - radial ** 2));
    entity.velocity = [-radial * direction[0] - tangent * Math.sin(az), -radial * direction[1], -radial * direction[2] + tangent * Math.cos(az)];
    world.add(entity);
  };
  if (scene.kind === 'legacy') {
    for (let i = 0; i < scene.data.length; i += 8) {
      const [bearing, range, depth, rpm, blades, tonal, cav, velocity] = scene.data.slice(i, i + 8);
      const target = new AcousticEntity(`contact-${i / 8}`);
      target.rpm = rpm;
      target.bladeCount = blades;
      target.sourceLevels.tonal = [tonal];
      target.cavitation = cav;
      place(target, bearing, range, depth, velocity);
    }
  } else {
    scene.targets.forEach((t, i) => {
      const target = new AcousticEntity(`contact-${i}`, 'surface');
      target.sourceProfileId = t.sourceProfileId;
      target.tonalOverlayId = t.tonalOverlayId;
      target.lengthM = t.lengthM;
      target.rpm = t.shaftRpm;
      target.bladeCount = t.bladeCount;
      place(target, t.bearingDeg, t.rangeM, t.depthM, t.relativeVelocityMs, t.speedKn * 1852 / 3600);
    });
  }
  return world;
}

/** TS 절대 좌표(동/아래/남) → 자함 기준 Rust 방위/수심/접근 속도. */
export function encodeWorld(world: World) {
  const ownship = world.get(world.ownshipId);
  if (!ownship) throw new Error('자함이 없는 월드입니다');
  const legacy: number[] = [];
  const profiled: number[] = [];
  for (const target of world.all()) {
    if (target.id === ownship.id) continue;
    const delta = target.position.map((v, i) => v - ownship.position[i]);
    const range = Math.max(1, Math.hypot(...delta));
    const angle = (Math.atan2(delta[2], delta[0]) - ownship.heading) * 180 / Math.PI;
    const bearing = ((angle % 360) + 360) % 360;
    const radial = -delta.reduce((sum, v, i) => sum + v * (target.velocity[i] - ownship.velocity[i]), 0) / range;
    if (target.sourceProfileId === 'legacy-generic') {
      legacy.push(bearing, range, target.position[1], target.rpm, target.bladeCount, target.sourceLevels.tonal[0], target.cavitation, radial);
    } else {
      profiled.push(bearing, range, target.position[1], MERCHANT_PROFILE_CODES[target.sourceProfileId],
        Math.hypot(...target.velocity) * 3600 / 1852, target.lengthM, radial,
        target.tonalOverlayId ? MERCHANT_TONAL_OVERLAY_CODES[target.tonalOverlayId] : 0, target.rpm, target.bladeCount);
    }
  }
  return { type: 'worldScene' as const, targets: new Float32Array(legacy), profiled: new Float32Array(profiled), receiverDepthM: ownship.position[1], contacts: legacy.length / 8 + profiled.length / 10 };
}
