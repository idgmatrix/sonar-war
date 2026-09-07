import type { ProfiledTargetV2 } from './sourceProfiles.ts';

export type DemoScene =
  | {
      label: string;
      kind: 'legacy';
      data: number[];
      beamAzimuthDeg: number;
    }
  | {
      label: string;
      kind: 'profiledV2';
      targets: ProfiledTargetV2[];
      beamAzimuthDeg: number;
    };

/**
 * M2 청감 검증용 장면.
 *
 * 일반 Source의 blade-rate 톤은 실제 물리 주파수라 대부분 저주파에 놓인다. 비교 장면은
 * 같은 물리식을 사용하되 비교용 준위·속도를 강조한다(C등급 합성 데모).
 * 저주파 재생이 어려운 장치에서는 별도 청취 보조를 사용할 수 있다.
 */
export const DEMO_SCENES: Record<string, DemoScene> = {
  demo: {
    label: 'DEMO · 3 targets',
    kind: 'legacy',
    data: [
      45, 3000, 50, 90, 5, 150, 0.3, 5,
      300, 8000, 200, 70, 4, 145, 0.1, -3,
      0, 1500, 30, 110, 6, 155, 0.6, 0,
    ],
    beamAzimuthDeg: 45,
  },
  approach: {
    label: 'APPROACH · Doppler +50 m/s',
    kind: 'legacy',
    data: [45, 1000, 40, 180, 6, 165, 0.05, 50],
    beamAzimuthDeg: 45,
  },
  recede: {
    label: 'RECEDING · Doppler −50 m/s',
    kind: 'legacy',
    data: [45, 1000, 40, 180, 6, 165, 0.05, -50],
    beamAzimuthDeg: 45,
  },
  rpmUp: {
    label: 'RPM 180 · 18 Hz blade rate',
    kind: 'legacy',
    data: [45, 1000, 40, 180, 6, 165, 0.05, 0],
    beamAzimuthDeg: 45,
  },
  rpmDown: {
    label: 'RPM 60 · 6 Hz blade rate',
    kind: 'legacy',
    data: [45, 1000, 40, 60, 6, 165, 0.05, 0],
    beamAzimuthDeg: 45,
  },
  merchant: {
    label: 'MERCHANT · measured 140 RPM analog',
    kind: 'profiledV2',
    targets: [
      {
        bearingDeg: 45,
        rangeM: 10000,
        depthM: 6,
        sourceProfileId: 'merchant-bulker-jomopans-echo',
        speedKn: 16,
        lengthM: 172.9,
        relativeVelocityMs: 0,
        tonalOverlayId: 'overseas-harriette-140rpm',
        shaftRpm: 140,
        bladeCount: 4,
      },
    ],
    beamAzimuthDeg: 45,
  },
};
