/**
 * AcousticEntity — 모든 잠수함/표적/어뢰/미끼가 공유하는 단일 음향 엔티티.
 *
 * 설계 원칙 (docs/개발 계획.md §1, §4):
 *  - "누가 조종하는지"(로컬/AI/원격) 엔진은 모른다.
 *  - 센서/렌더 파이프라인은 AcousticEntity[]만 받는다.
 *  - exportState/importState를 첫날부터 구현해 멀티플레이 레이어를 얹을 준비를 한다.
 *
 * 좌표계: x (동), depth (+하향), z (남). 단위: m.
 */

import type { SourceProfileId } from '../../dsp/sourceProfiles.ts';

export type EntityKind = 'sub' | 'surface' | 'torpedo' | 'decoy';

export interface AcousticEntityState {
  id: string;
  kind: EntityKind;
  /** [x, depth, z] (m, 수심은 +down) */
  position: [number, number, number];
  /** [vx, vdepth, vz] (m/s) */
  velocity: [number, number, number];
  /** 방위각 (rad, 0 = +x, 시계방향) */
  heading: number;
  /** 스크루 회전수 (RPM) */
  rpm: number;
  sourceProfileId: SourceProfileId;
  lengthM: number;
  bladeCount: number;
  /** 0~1 광대역 캐비테이션 레벨 */
  cavitation: number;
  /** Hz — LOFAR 토널 (기계 고조파) */
  tonals: number[];
  /** Hz — DEMON 복조 주파수 (RPM × 엽수 / 60) */
  bladeRate: number;
  /** dB */
  sourceLevels: { broadband: number; tonal: number[] };
  towedArrayDeployed: boolean;
}

/**
 * 네트워크 페이로드 (2~5Hz, 수십 바이트) — WebRTC 문서의 acousticState와 1:1 대응.
 * 정적 메타(kind, id)는 제외하고 상태 변수만 직렬화한다.
 */
export interface AcousticEntityPayload {
  pos: [number, number, number];
  vel: [number, number, number];
  hdg: number;
  spd: number; // m/s (스칼라, 검증/디버깅용)
  rpm: number;
  profile: SourceProfileId;
  lengthM: number;
  blades: number;
  cav: number;
  tonals: number[];
  bladeRate: number;
  sl: { bb: number; tn: number[] }; // sourceLevels 압축
  towed: boolean;
}

export class AcousticEntity {
  id: string;
  kind: EntityKind;
  position: [number, number, number];
  velocity: [number, number, number];
  heading: number;
  rpm: number;
  sourceProfileId: SourceProfileId;
  lengthM: number;
  bladeCount: number;
  cavitation: number;
  tonals: number[];
  bladeRate: number;
  sourceLevels: { broadband: number; tonal: number[] };
  towedArrayDeployed: boolean;

  constructor(id: string, kind: EntityKind = 'sub') {
    this.id = id;
    this.kind = kind;
    this.position = [0, 150, 0];
    this.velocity = [0, 0, 0];
    this.heading = 0;
    this.rpm = 90;
    this.sourceProfileId = 'legacy-generic';
    this.lengthM = 100;
    this.bladeCount = 7;
    this.cavitation = 0.0;
    this.tonals = [60, 120, 180];
    this.bladeRate = (90 * 7) / 60; // 7엽 스크루 기준
    this.sourceLevels = { broadband: 150, tonal: [120, 110, 100] };
    this.towedArrayDeployed = false;
  }

  /** 스크루 엽수 변경 시 bladeRate 재계산 */
  setBladeCount(count: number): void {
    this.bladeCount = count;
    this.bladeRate = (this.rpm * count) / 60;
  }

  /** 네트워크/AI로 보낼 압축 상태 추출 */
  exportState(): AcousticEntityPayload {
    const spd = Math.hypot(this.velocity[0], this.velocity[1], this.velocity[2]);
    return {
      pos: [...this.position],
      vel: [...this.velocity],
      hdg: this.heading,
      spd,
      rpm: this.rpm,
      profile: this.sourceProfileId,
      lengthM: this.lengthM,
      blades: this.bladeCount,
      cav: this.cavitation,
      tonals: [...this.tonals],
      bladeRate: this.bladeRate,
      sl: { bb: this.sourceLevels.broadband, tn: [...this.sourceLevels.tonal] },
      towed: this.towedArrayDeployed,
    };
  }

  /** 네트워크/AI로부터 상태 주입 */
  importState(data: AcousticEntityPayload): void {
    this.position = [...data.pos];
    this.velocity = [...data.vel];
    this.heading = data.hdg;
    this.rpm = data.rpm;
    this.sourceProfileId = data.profile;
    this.lengthM = data.lengthM;
    this.bladeCount = data.blades;
    this.cavitation = data.cav;
    this.tonals = [...data.tonals];
    this.bladeRate = data.bladeRate;
    this.sourceLevels = { broadband: data.sl.bb, tonal: [...data.sl.tn] };
    this.towedArrayDeployed = data.towed;
  }

  /** 완전한 상태 스냅샷 (시뮬레이션 세이브/복원용) */
  snapshot(): AcousticEntityState {
    return {
      id: this.id,
      kind: this.kind,
      position: [...this.position],
      velocity: [...this.velocity],
      heading: this.heading,
      rpm: this.rpm,
      sourceProfileId: this.sourceProfileId,
      lengthM: this.lengthM,
      bladeCount: this.bladeCount,
      cavitation: this.cavitation,
      tonals: [...this.tonals],
      bladeRate: this.bladeRate,
      sourceLevels: {
        broadband: this.sourceLevels.broadband,
        tonal: [...this.sourceLevels.tonal],
      },
      towedArrayDeployed: this.towedArrayDeployed,
    };
  }

  static fromSnapshot(s: AcousticEntityState): AcousticEntity {
    const e = new AcousticEntity(s.id, s.kind);
    e.position = [...s.position];
    e.velocity = [...s.velocity];
    e.heading = s.heading;
    e.rpm = s.rpm;
    e.sourceProfileId = s.sourceProfileId;
    e.lengthM = s.lengthM;
    e.bladeCount = s.bladeCount;
    e.cavitation = s.cavitation;
    e.tonals = [...s.tonals];
    e.bladeRate = s.bladeRate;
    e.sourceLevels = {
      broadband: s.sourceLevels.broadband,
      tonal: [...s.sourceLevels.tonal],
    };
    e.towedArrayDeployed = s.towedArrayDeployed;
    return e;
  }
}
