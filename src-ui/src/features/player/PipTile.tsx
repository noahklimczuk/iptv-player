/**
 * The frame around the small picture (README §6.2, §14.1 `P`).
 *
 * **There is no video in here.** The picture is the main player's own surface, moved
 * into a corner by the host — so what this draws is a border, a label and the two
 * controls, aligned with where mpv actually put the frames. The rectangle comes from the
 * host for that reason: a tile the UI positioned itself would be a frame that does not
 * line up with its own video the moment the two disagree about rounding.
 *
 * It also has to punch a hole. The shell is opaque while PiP is on — the viewer is
 * reading a page, not watching full-screen — so the only way the picture shows through
 * is for nothing to be painted over that rectangle. That is the `backdrop` below: four
 * opaque bands around the tile rather than one translucent sheet over it, because a
 * transparent div does not erase what is behind it, it just fails to cover it.
 */
import type { PipView } from '@shared/ipc';
import { Icon } from '@/components/Icon';

/** Physical pixels to CSS pixels, read live so a move between displays is handled. */
function cssRect(rect: PipView['rect'], dpr: number) {
  return {
    left: rect.x / dpr,
    top: rect.y / dpr,
    width: rect.width / dpr,
    height: rect.height / dpr,
  };
}

export function PipTile({
  view,
  title,
  dpr,
  onExpand,
  onClose,
  onCycleCorner,
}: {
  view: PipView;
  /** What is playing, so the tile says what it is without the OSD. */
  title: string | null;
  dpr: number;
  onExpand: () => void;
  onClose: () => void;
  onCycleCorner: () => void;
}) {
  if (!view.enabled || view.rect.width === 0) return null;
  const box = cssRect(view.rect, dpr);

  const band = (style: React.CSSProperties): React.CSSProperties => ({
    position: 'fixed',
    background: 'var(--bg)',
    zIndex: 58,
    pointerEvents: 'none',
    ...style,
  });

  return (
    <>
      {/*
        * The hole. Four opaque bands around the tile, so the page behind stays readable
        * and the only see-through rectangle in the window is the one with video in it.
        */}
      <div style={band({ left: 0, right: 0, top: 0, height: box.top })} />
      <div style={band({ left: 0, right: 0, top: box.top + box.height, bottom: 0 })} />
      <div style={band({ left: 0, width: box.left, top: box.top, height: box.height })} />
      <div
        style={band({
          left: box.left + box.width,
          right: 0,
          top: box.top,
          height: box.height,
        })}
      />

      <div
        data-testid="pip-tile"
        data-corner={view.corner}
        style={{
          position: 'fixed',
          ...box,
          zIndex: 59,
          borderRadius: 'var(--r-md)',
          border: '1px solid color-mix(in srgb, var(--text) 24%, transparent)',
          boxShadow: 'var(--shadow-4)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          // Transparent, because the picture is behind the WebView. Anything painted
          // here is painted over the video.
          background: 'transparent',
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 4,
            padding: '4px 6px',
            background: 'color-mix(in srgb, var(--bg) 62%, transparent)',
          }}
        >
          <span
            style={{
              flex: 1,
              fontSize: 11,
              fontWeight: 600,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {title ?? 'Playing'}
          </span>
          <button
            type="button"
            onClick={onCycleCorner}
            aria-label="Move the small picture to the next corner"
            data-testid="pip-corner"
            style={pipButton}
          >
            <Icon name="layers" size={13} />
          </button>
          <button
            type="button"
            onClick={onExpand}
            aria-label="Expand the small picture"
            data-testid="pip-expand"
            style={pipButton}
          >
            <Icon name="fullscreen" size={13} />
          </button>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close the small picture"
            data-testid="pip-close"
            style={pipButton}
          >
            <Icon name="close" size={13} />
          </button>
        </div>
      </div>
    </>
  );
}

const pipButton: React.CSSProperties = {
  display: 'grid',
  placeItems: 'center',
  width: 20,
  height: 20,
  border: 'none',
  borderRadius: 'var(--r-sm)',
  background: 'transparent',
  color: 'var(--text)',
  cursor: 'pointer',
};
