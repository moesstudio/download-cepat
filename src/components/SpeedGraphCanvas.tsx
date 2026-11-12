import React, { useEffect, useRef } from 'react';
import { useI18n } from '../i18n/index.tsx';

interface SpeedGraphCanvasProps {
  currentSpeedBytesPerSec: number;
  width?: number;
  height?: number;
}

export const SpeedGraphCanvas: React.FC<SpeedGraphCanvasProps> = ({
  currentSpeedBytesPerSec,
  width = 500,
  height = 100,
}) => {
  const { t } = useI18n();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const historyRef = useRef<number[]>(new Array(40).fill(0));

  useEffect(() => {
    // Shift history and append new reading
    historyRef.current.shift();
    historyRef.current.push(currentSpeedBytesPerSec);

    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);

    // Clear background
    ctx.clearRect(0, 0, width, height);

    const data = historyRef.current;
    const rawMax = Math.max(...data, 1024 * 1024);
    const maxVal = rawMax * 1.15; // 15% headroom so line doesn't clip at top

    // Draw horizontal grid lines
    ctx.strokeStyle = 'rgba(100, 116, 139, 0.15)';
    ctx.lineWidth = 1;
    for (let i = 1; i <= 3; i++) {
      const y = (height / 4) * i;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }

    // Build curve path
    const step = width / (data.length - 1);
    ctx.beginPath();
    ctx.moveTo(0, height - (data[0] / maxVal) * (height - 10));

    for (let i = 1; i < data.length; i++) {
      const x = i * step;
      const y = height - (data[i] / maxVal) * (height - 10);
      ctx.lineTo(x, y);
    }

    // Fill area under curve
    const gradient = ctx.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, 'rgba(29, 78, 216, 0.35)');
    gradient.addColorStop(1, 'rgba(29, 78, 216, 0.0)');

    ctx.lineTo(width, height);
    ctx.lineTo(0, height);
    ctx.closePath();
    ctx.fillStyle = gradient;
    ctx.fill();

    // Draw line stroke
    ctx.beginPath();
    ctx.moveTo(0, height - (data[0] / maxVal) * (height - 10));
    for (let i = 1; i < data.length; i++) {
      const x = i * step;
      const y = height - (data[i] / maxVal) * (height - 10);
      ctx.lineTo(x, y);
    }
    ctx.strokeStyle = '#1d4ed8';
    ctx.lineWidth = 2;
    ctx.stroke();

    // Display scale peak unobtrusively
    const peakMb = (rawMax / (1024 * 1024)).toFixed(1);
    ctx.fillStyle = 'rgba(148, 163, 184, 0.8)';
    ctx.font = '500 10px var(--font-mono, monospace)';
    ctx.textAlign = 'right';
    ctx.fillText(t('graph.peak', { mb: peakMb }), width - 8, 14);
  }, [currentSpeedBytesPerSec, width, height, t]);

  return (
    <div className="w-full bg-surface rounded-md border border-border p-2 overflow-hidden">
      <div className="flex justify-between items-center mb-1 text-xs text-text-muted">
        <span className="font-medium">{t('graph.title')}</span>
        <span className="font-mono text-accent font-semibold">
          {(currentSpeedBytesPerSec / (1024 * 1024)).toFixed(2)} MB/s
        </span>
      </div>
      <canvas
        ref={canvasRef}
        style={{ width: '100%', height: `${height}px` }}
        className="block"
      />
    </div>
  );
};
