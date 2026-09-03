import { describe, it, expect } from 'vitest';
import { AcousticEntity } from '../src/core/entity/acousticEntity.ts';

describe('AcousticEntity 직렬화 (M0 완료 기준)', () => {
  it('exportState → importState 왕복이 상태를 보존한다', () => {
    const a = new AcousticEntity('sub-1', 'sub');
    a.position = [1234.5, 250.0, -777.25];
    a.velocity = [2.5, -0.3, 1.1];
    a.heading = 1.5708;
    a.rpm = 142;
    a.sourceProfileId = 'merchant-bulker-jomopans-echo';
    a.lengthM = 211;
    a.setBladeCount(5);
    a.cavitation = 0.67;
    a.tonals = [50, 100, 150, 200];
    a.bladeRate = 16.5;
    a.sourceLevels = { broadband: 155, tonal: [130, 120, 110, 100] };
    a.towedArrayDeployed = true;

    const payload = a.exportState();

    const b = new AcousticEntity('sub-2', 'sub');
    b.importState(payload);

    expect(b.position).toEqual(a.position);
    expect(b.velocity).toEqual(a.velocity);
    expect(b.heading).toBeCloseTo(a.heading);
    expect(b.rpm).toBe(a.rpm);
    expect(b.sourceProfileId).toBe(a.sourceProfileId);
    expect(b.lengthM).toBe(a.lengthM);
    expect(b.bladeCount).toBe(a.bladeCount);
    expect(b.cavitation).toBeCloseTo(a.cavitation);
    expect(b.tonals).toEqual(a.tonals);
    expect(b.bladeRate).toBeCloseTo(a.bladeRate);
    expect(b.sourceLevels.broadband).toBe(a.sourceLevels.broadband);
    expect(b.sourceLevels.tonal).toEqual(a.sourceLevels.tonal);
    expect(b.towedArrayDeployed).toBe(true);
  });

  it('exportState가 원본을 변형하지 않는다 (불변 복사)', () => {
    const a = new AcousticEntity('sub-1');
    a.position = [1, 2, 3];
    const payload = a.exportState();
    payload.pos[0] = 999;
    expect(a.position[0]).toBe(1);
  });

  it('importState가 payload를 변형하지 않는다 (불변 복사)', () => {
    const a = new AcousticEntity('sub-1');
    const payload = a.exportState();
    const b = new AcousticEntity('sub-2');
    b.importState(payload);
    b.position[0] = 42;
    expect(payload.pos[0]).toBe(a.position[0]);
  });

  it('snapshot → fromSnapshot 왕복이 정적 메타(id/kind)까지 보존한다', () => {
    const a = new AcousticEntity('torp-9', 'torpedo');
    a.rpm = 200;
    const restored = AcousticEntity.fromSnapshot(a.snapshot());
    expect(restored.id).toBe('torp-9');
    expect(restored.kind).toBe('torpedo');
    expect(restored.rpm).toBe(200);
  });

  it('setBladeCount가 bladeRate를 RPM×엽수/60으로 재계산한다', () => {
    const a = new AcousticEntity('sub-1');
    a.rpm = 120;
    a.setBladeCount(7);
    expect(a.bladeRate).toBeCloseTo((120 * 7) / 60);
  });
});
