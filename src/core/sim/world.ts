/**
 * 월드 상태 — 시뮬레이션이 소유하는 모든 AcousticEntity.
 *
 * 센서/렌더/네트워크는 이 목록만 참조한다 (docs/개발 계획.md §1).
 */
import { AcousticEntity } from '../entity/acousticEntity.ts';

export class World {
  entities = new Map<string, AcousticEntity>();
  /** 자함 id (멀티플레이 시 로컬 플레이어) */
  ownshipId = 'ownship';

  add(entity: AcousticEntity): void {
    this.entities.set(entity.id, entity);
  }

  remove(id: string): void {
    this.entities.delete(id);
  }

  get(id: string): AcousticEntity | undefined {
    return this.entities.get(id);
  }

  /** 모든 엔티티 배열 (센서/렌더 입력용) */
  all(): AcousticEntity[] {
    return Array.from(this.entities.values());
  }
}

/** M0 데모 월드: 자함 + 스크립트 더미 타겟 1기 */
export function createDemoWorld(): World {
  const world = new World();
  const ownship = new AcousticEntity('ownship', 'sub');
  ownship.position = [0, 150, 0];
  world.add(ownship);

  const target = new AcousticEntity('target-1', 'sub');
  target.position = [3000, 200, 500]; // 동쪽 3km, 수심 200m
  target.heading = Math.PI; // 자함 방향으로 접근
  target.velocity = [2.0, 0, 0];
  target.rpm = 120;
  target.cavitation = 0.3;
  world.add(target);

  return world;
}
