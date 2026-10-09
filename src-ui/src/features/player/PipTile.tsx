/**
 * The frame around the small picture (README §6.2, §14.1 `P`).
 *
 * **There is no video in here.** The picture is the main player's own surface, moved
 * into a corner by the host — so what this draws is a border, a label and the two
 * controls, aligned with where mpv actually put the frames. The rectangle comes from the
 * host for that reason: a tile the UI positioned itself would be a frame that does not
 * line up with its own video the moment the two disagree about rounding.
 *
 * **It does not punch the hole.** It used to try, with four opaque bands around the tile
 * at `z-index: 58`, and that could not work in two ways at once: the bands sat above the
 * page, so they covered the thing PiP exists to let you read, and the shell underneath
 * was still painting, so the rectangle they left showed shell rather than video. The
 * small picture has never displayed anything as a result.
 *
 * The hole is cut by clipping the app shell (`holeClipPath` in `SurfaceHole`), which
 * removes the rectangle from the layer instead of trying to cover around it. This tile is
 * portalled to `document.body` so that it is *outside* that clipped subtree — its frame
 * and controls sit inside the hole by definition, and would be clipped away with
 * everything else.
 */
import { createPortal } from 'react-dom';
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

  return createPortal(
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
      </div>,
    document.body,
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
