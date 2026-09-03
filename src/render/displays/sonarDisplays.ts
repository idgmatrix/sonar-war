import {
  bearingFromRatio,
  clamp01,
  frequencyBin,
  frequencyToRatio,
  harmonicFrequencies,
} from './sonarMath.ts';

const COLORS = {
  abyss: '#020706',
  grid: '#173c2c',
  trace: '#65ff96',
  cursor: '#ffd166',
  text: '#d6e8dc',
};

function intensityColor(value: number): string {
  const t = clamp01(value);
  const red = Math.round(10 + t * 72);
  const green = Math.round(25 + t * 230);
  const blue = Math.round(19 + t * 112);
  return `rgb(${red} ${green} ${blue})`;
}

abstract class WaterfallDisplay {
  protected readonly ctx: CanvasRenderingContext2D;
  protected readonly history: HTMLCanvasElement;
  protected readonly historyCtx: CanvasRenderingContext2D;

  constructor(protected readonly canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Canvas 2D context unavailable');
    this.ctx = ctx;
    this.history = document.createElement('canvas');
    this.history.width = canvas.width;
    this.history.height = canvas.height;
    const historyCtx = this.history.getContext('2d');
    if (!historyCtx) throw new Error('Offscreen Canvas 2D context unavailable');
    this.historyCtx = historyCtx;
    this.historyCtx.fillStyle = COLORS.abyss;
    this.historyCtx.fillRect(0, 0, canvas.width, canvas.height);
  }

  protected pushRow(sampleAt: (ratio: number) => number): void {
    const { width, height } = this.history;
    this.historyCtx.drawImage(this.history, 0, 0, width, height - 1, 0, 1, width, height - 1);
    for (let x = 0; x < width; x += 1) {
      this.historyCtx.fillStyle = intensityColor(sampleAt(x / Math.max(1, width - 1)));
      this.historyCtx.fillRect(x, 0, 1, 1);
    }
  }

  protected paintHistory(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.ctx.drawImage(this.history, 0, 0);
  }

  protected verticalGrid(divisions: number, formatter: (index: number) => string): void {
    const { width, height } = this.canvas;
    this.ctx.save();
    this.ctx.font = '18px "Bahnschrift", "DejaVu Sans Mono", monospace';
    this.ctx.textBaseline = 'top';
    for (let i = 0; i <= divisions; i += 1) {
      const x = Math.round((i / divisions) * (width - 1));
      this.ctx.strokeStyle = COLORS.grid;
      this.ctx.beginPath();
      this.ctx.moveTo(x + 0.5, 0);
      this.ctx.lineTo(x + 0.5, height);
      this.ctx.stroke();
      this.ctx.fillStyle = COLORS.text;
      this.ctx.fillText(formatter(i), Math.min(x + 5, width - 55), 8);
    }
    this.ctx.restore();
  }
}

export class LofarDisplay extends WaterfallDisplay {
  private fundamentalHz = 10;

  constructor(canvas: HTMLCanvasElement, private readonly maxFrequencyHz = 500) {
    super(canvas);
  }

  setFundamental(frequencyHz: number): void {
    this.fundamentalHz = frequencyHz;
    this.renderOverlay();
  }

  update(levelsDb: Float32Array, sampleRate: number): void {
    this.pushRow((ratio) => {
      const bin = frequencyBin(ratio * this.maxFrequencyHz, sampleRate, levelsDb.length);
      return (levelsDb[bin] + 110) / 80;
    });
    this.renderOverlay();
  }

  private renderOverlay(): void {
    this.paintHistory();
    this.verticalGrid(5, (index) => `${(index * this.maxFrequencyHz) / 5}`);
    this.ctx.save();
    this.ctx.strokeStyle = COLORS.cursor;
    this.ctx.fillStyle = COLORS.cursor;
    this.ctx.font = '16px "Bahnschrift", "DejaVu Sans Mono", monospace';
    for (const frequency of harmonicFrequencies(this.fundamentalHz, this.maxFrequencyHz)) {
      const x = frequencyToRatio(frequency, this.maxFrequencyHz) * this.canvas.width;
      this.ctx.globalAlpha = frequency === this.fundamentalHz ? 0.95 : 0.42;
      this.ctx.beginPath();
      this.ctx.moveTo(x + 0.5, 0);
      this.ctx.lineTo(x + 0.5, this.canvas.height);
      this.ctx.stroke();
    }
    this.ctx.globalAlpha = 1;
    this.ctx.fillText(`H₁ ${this.fundamentalHz.toFixed(1)} Hz`, 12, this.canvas.height - 28);
    this.ctx.restore();
  }
}

export class BassDisplay extends WaterfallDisplay {
  private bearing = 0;

  setBearing(bearing: number): void {
    this.bearing = ((bearing % 360) + 360) % 360;
    this.renderOverlay();
  }

  bearingAtClientX(clientX: number): number {
    const rect = this.canvas.getBoundingClientRect();
    return bearingFromRatio((clientX - rect.left) / rect.width);
  }

  update(levelsDb: Float32Array): void {
    this.pushRow((ratio) => {
      const bin = Math.min(levelsDb.length - 1, Math.floor(ratio * levelsDb.length));
      return (levelsDb[bin] + 100) / 80;
    });
    this.renderOverlay();
  }

  private renderOverlay(): void {
    this.paintHistory();
    this.verticalGrid(8, (index) => `${String(index * 45).padStart(3, '0')}°`);
    const x = (this.bearing / 360) * this.canvas.width;
    this.ctx.save();
    this.ctx.strokeStyle = COLORS.cursor;
    this.ctx.lineWidth = 2;
    this.ctx.beginPath();
    this.ctx.moveTo(x + 0.5, 0);
    this.ctx.lineTo(x + 0.5, this.canvas.height);
    this.ctx.stroke();
    this.ctx.fillStyle = COLORS.cursor;
    this.ctx.beginPath();
    this.ctx.moveTo(x - 8, 0);
    this.ctx.lineTo(x + 8, 0);
    this.ctx.lineTo(x, 12);
    this.ctx.closePath();
    this.ctx.fill();
    this.ctx.restore();
  }
}
