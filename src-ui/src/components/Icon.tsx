/** Inline SVG icon set — no icon-font dependency, themeable via currentColor. */
import type { CSSProperties } from 'react';

export const ICON_PATHS: Record<string, string> = {
  play: 'M5 3v18l14-9z',
  pause: 'M6 5h4v14H6zm8 0h4v14h-4z',
  stop: 'M6 6h12v12H6z',
  plus: 'M12 5v14M5 12h14',
  check: 'M4 12l5 5L20 6',
  info: 'M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z',
  chevronRight: 'M9 6l6 6-6 6',
  chevronLeft: 'M15 6l-6 6 6 6',
  chevronDown: 'M6 9l6 6 6-6',
  search: 'M11 4a7 7 0 100 14 7 7 0 000-14zM20 20l-4-4',
  // Two arcs with arrowheads — the usual "go and ask the provider again" glyph.
  refresh: 'M20 11a8 8 0 10-.6 4M20 5v6h-6',
  home: 'M3 11l9-8 9 8M6 10v10h12V10',
  tv: 'M3 5h18v11H3zM8 20h8M12 16v4',
  grid: 'M3 4h18v4H3zM3 10h8v4H3zM13 10h8v4h-8zM3 16h18v4H3z',
  film: 'M3 4h18v16H3zM7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4',
  stack: 'M4 6h16v12H4zM8 3h12M6 21h12',
  settings:
    'M12 9a3 3 0 100 6 3 3 0 000-6zM12 5.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13z' +
    'M12 3v2.5M12 18.5v2.5M3 12h2.5M18.5 12h2.5' +
    'M5.64 5.64l1.76 1.76M16.6 16.6l1.76 1.76M18.36 5.64L16.6 7.4M7.4 16.6l-1.76 1.76',
  volume: 'M11 5L6 9H3v6h3l5 4V5zM15.5 9a4 4 0 010 6M18.5 6a8 8 0 010 12',
  volumeOff: 'M11 5L6 9H3v6h3l5 4V5zM17 9l4 6M21 9l-4 6',
  fullscreen: 'M3 9V3h6M21 9V3h-6M3 15v6h6M21 15v6h-6',
  record: 'M12 7a5 5 0 100 10 5 5 0 000-10z',
  clock: 'M12 3a9 9 0 100 18 9 9 0 000-18zM12 7v5l3 2',
  bell: 'M18 8a6 6 0 10-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4',
  star: 'M12 3l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5-5.8-3-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z',
  thumbUp: 'M7 10v11H3V10zM7 10l5-7a2 2 0 013 2l-1 5h5a2 2 0 012 2.4l-1.6 7A2 2 0 0117 21H7',
  close: 'M6 6l12 12M18 6L6 18',
  skip: 'M5 4l10 8-10 8zM19 4v16',
  back10: 'M4 11a8 8 0 11.6 4M4 5v6h6',
  forward10: 'M20 11a8 8 0 10-.6 4M20 5v6h-6',
  subtitles: 'M3 5h18v14H3zM7 11h4M13 11h4M7 15h10',
  audio: 'M12 3v18M8 7v10M16 7v10M4 10v4M20 10v4',
  pip: 'M3 5h18v14H3zM12 12h7v5h-7z',
  cast: 'M3 16a4 4 0 014 4M3 12a8 8 0 018 8M3 8a12 12 0 0112 12M3 4h18v14h-6',
  layers: 'M12 4.5l9 5-9 5-9-5zM3 14.5l9 5 9-5',
  sparkle: 'M12 5l1.8 5.2L19 12l-5.2 1.8L12 19l-1.8-5.2L5 12l5.2-1.8z',
  heart: 'M12 18.5s-7-4.4-7-9a4 4 0 017-2.6A4 4 0 0119 9.5c0 4.6-7 9-7 9z',
  keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
  alert: 'M12 9v5M12 17h.01M10.3 3.9L1.8 18a2 2 0 001.7 3h17a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z',
};

export type IconName = keyof typeof ICON_PATHS | string;

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
  const d = ICON_PATHS[name] ?? ICON_PATHS.info!;
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
