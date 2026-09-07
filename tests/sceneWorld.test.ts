import { describe, it, expect } from 'vitest';
import { DEMO_SCENES } from '../src/dsp/demoScenes.ts';
import { worldFromScene, encodeWorld } from '../src/dsp/sceneWorld.ts';

describe('월드 → DSP 경계', () => {
  it('모든 프리셋을 월드를 통해 전달해도 물리 조건과 접촉 수가 보존된다', () => {
    for (const scene of Object.values(DEMO_SCENES)) {
      const world = worldFromScene(scene);
      const encoded = encodeWorld(world);
      expect(encoded.contacts).toBe(world.all().length - 1);
      expect(encoded.receiverDepthM).toBe(150);
      if (scene.kind === 'legacy') {
        scene.data.forEach((value, i) => expect(encoded.targets[i]).toBeCloseTo(value, 3));
      } else {
        expect(encoded.profiled[1]).toBeCloseTo(scene.targets[0].rangeM, 3);
        expect(encoded.profiled[4]).toBeCloseTo(scene.targets[0].speedKn, 4);
        expect(encoded.profiled[6]).toBeCloseTo(scene.targets[0].relativeVelocityMs, 4);
        expect(encoded.profiled[7]).toBe(1);
      }
    }
  });
  it('자함 방위와 속도를 바꾸면 상대 방위와 접근 속도가 변한다', () => {
    const world = worldFromScene(DEMO_SCENES.rpmUp);
    const ownship = world.get(world.ownshipId)!;
    ownship.heading = Math.PI / 4;
    ownship.velocity = [10, 0, 0];
    const encoded = encodeWorld(world);
    expect(encoded.targets[0]).toBeCloseTo(0, 4);
    expect(encoded.targets[7]).toBeGreaterThan(7);
    expect(encoded.targets[7]).toBeLessThan(7.1);
  });
});
