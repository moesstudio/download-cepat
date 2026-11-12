import '@testing-library/jest-dom';

// Mock Canvas getContext for jsdom
HTMLCanvasElement.prototype.getContext = () => ({
  clearRect: () => {},
  beginPath: () => {},
  moveTo: () => {},
  lineTo: () => {},
  stroke: () => {},
  fill: () => {},
  closePath: () => {},
  createLinearGradient: () => ({
    addColorStop: () => {},
  }),
  fillText: () => {},
  scale: () => {},
} as any);
