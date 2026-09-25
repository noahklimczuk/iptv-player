/** Inline SVG icon set — no icon-font dependency, themeable via currentColor. */
import type { CSSProperties } from 'react';

const P: Record<string, string> = {
  play: 'M8 5v14l11-7z',
  pause: 'M6 5h4v14H6zm8 0h4v14h-4z',
  stop: 'M6 6h12v12H6z',
  plus: 'M12 5v14M5 12h14',
  check: 'M4 12l5 5L20 6',
  info: 'M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z',
  chevronRight: 'M9 6l6 6-6 6',
  chevronLeft: 'M15 6l-6 6 6 6',
  chevronDown: 'M6 9l6 6 6-6',
  search: 'M11 4a7 7 0 100 14 7 7 0 000-14zM20 20l-4-4',
  home: 'M3 11l9-8 9 8M6 10v10h12V10',
  tv: 'M3 5h18v11H3zM8 20h8M12 16v4',
  grid: 'M3 4h18v4H3zM3 10h8v4H3zM13 10h8v4h-8zM3 16h18v4H3z',
  film: 'M3 4h18v16H3zM7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4',
  stack: 'M4 6h16v12H4zM8 3h12M6 21h12',
  settings: 'M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.6 1.6 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.6 1.6 0 00-1.8-.3 1.6 1.6 0 00-1 1.5V21a2 2 0 11-4 0v-.1A1.6 1.6 0 007 19.4a1.6 1.6 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.6 1.6 0 00.3-1.8 1.6 1.6 0 00-1.5-1H1a2 2 0 110-4h.1A1.6 1.6 0 002.6 9a1.6 1.6 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.6 1.6 0 001.8.3H7a1.6 1.6 0 001-1.5V3a2 2 0 114 0v.1a1.6 1.6 0 001 1.5 1.6 1.6 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.6 1.6 0 00-.3 1.8V9a1.6 1.6 0 001.5 1H21a2 2 0 110 4h-.1a1.6 1.6 0 00-1.5 1z',
  volume: 'M11 5L6 9H3v6h3l5 4V5z',
  volumeOff: 'M11 5L6 9H3v6h3l5 4V5zM17 9l4 6M21 9l-4 6',
  fullscreen: 'M3 9V3h6M21 9V3h-6M3 15v6h6M21 15v6h-6',
  record: 'M12 7a5 5 0 100 10 5 5 0 000-10z',
  clock: 'M12 3a9 9 0 100 18 9 9 0 000-18zM12 7v5l3 2',
  bell: 'M18 8a6 6 0 10-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4',
  star: 'M12 3l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5-5.8-3-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z',
  thumbUp: 'M7 10v11H3V10zM7 10l5-7a2 2 0 013 2l-1 5h5a2 2 0 012 2.4l-1.6 7A2 2 0 0117 21H7',
  close: 'M6 6l12 12M18 6L6 18',
  skip: 'M5 4l10 8-10 8zM19 4v16',
  back10: 'M11 4A8 8 0 103 12M11 4L7 1M11 4L7 7',
  forward10: 'M13 4a8 8 0 118 8M13 4l4-3M13 4l4 3',
  subtitles: 'M3 5h18v14H3zM7 11h4M13 11h4M7 15h10',
  audio: 'M12 3v18M8 7v10M16 7v10M4 10v4M20 10v4',
  pip: 'M3 5h18v14H3zM12 12h7v5h-7z',
  cast: 'M3 17a4 4 0 014 4M3 13a8 8 0 018 8M3 9a12 12 0 0112 12M3 5h18v14h-6',
  layers: 'M12 3l9 5-9 5-9-5zM3 13l9 5 9-5',
  sparkle: 'M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z',
  heart: 'M12 20s-7-4.4-7-9a4 4 0 017-2.6A4 4 0 0119 11c0 4.6-7 9-7 9z',
  keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
  alert: 'M12 9v5M12 17h.01M10.3 3.9L1.8 18a2 2 0 001.7 3h17a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z',
};

export type IconName = keyof typeof P | string;

export function Icon({
  name, size = 20, strokeWidth = 1.8, filled = false, style, className,
}: {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  filled?: boolean;
  style?: CSSProperties;
  className?: string;
}) {
  const d = P[name] ?? P.info!;
  return (
    <svg
      width={size} height={size} viewBox="0 0 24 24" aria-hidden="true"
      className={className}
      style={{ flexShrink: 0, display: 'block', ...style }}
      fill={filled ? 'currentColor' : 'none'}
      stroke={filled ? 'none' : 'currentColor'}
      strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round"
    >
      <path d={d} />
    </svg>
  );
}
