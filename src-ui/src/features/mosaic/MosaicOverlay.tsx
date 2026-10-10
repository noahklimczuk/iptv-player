/**
 * The chrome over an open mosaic (README §7.4).
 *
 * **There is no video in here.** Each tile's picture is a child window of the app's own
 * window, composited *behind* the transparent WebView by the host — so what this draws
 * is a label, a border and a hit target per tile, aligned with where mpv actually put
 * the frames. Which is why the rectangles come from the host rather than from CSS: a
 * grid the UI laid out itself would be chrome that does not line up with its own video
 * the moment the two disagree about rounding.
 *
 * The one conversion that has to happen here is pixels. `rect` is in *physical* pixels,
 * because that is what a window handle and `SetWindowPos` deal in; CSS is in logical
 * ones. On a 150% display those differ by half again, so a tile's border would sit a
 * third of the way into its neighbour without the `devicePixelRatio` divide below.
 */
import { useEffect, useState } from 'react';
import type { MosaicView } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { Icon } from '@/components/Icon';

/**
 * Physical pixels to CSS pixels.
 *
 * Read live rather than once: dragging a window to a display with another scale factor
 * changes it without the window being resized, and `AUDIT/test-report.md` §11 is a
 * two-monitor test precisely because that is a case this project has been caught by.
 */
function useDevicePixelRatio(): number {
  const [dpr, setDpr] = useState(() => window.devicePixelRatio || 1);
  useEffect(() => {
    const media = window.matchMedia(`(resolution: ${dpr}dppx)`);
    const onChange = () => setDpr(window.devicePixelRatio || 1);
    media.addEventListener('change', onChange);
    return () => media.removeEventListener('change', onChange);
  }, [dpr]);
  return dpr;
}

export function MosaicOverlay({
  view,
  onFocus,
  onPromote,
  onClose,
  onSave,
}: {
  view: MosaicView;
  onFocus: (index: number) => void;
  onPromote: (index: number) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  const dpr = useDevicePixelRatio();

  if (!view.open) return null;

  return (
    <div
      data-testid="mosaic-overlay"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 160,
        // Nothing here eats clicks except the tiles and the bar: the rest of the window
        // is video, and a transparent div over it that swallowed input would make the
        // picture feel dead.
        pointerEvents: 'none',
      }}
    >
      {view.tiles.map((tile) => (
        <button
          key={tile.index}
          type="button"
          data-testid={`mosaic-tile-${tile.index}`}
          data-focused={tile.focused ? 'true' : 'false'}
          aria-label={
            tile.name
              ? `Tile ${tile.index + 1}: ${tile.name}${tile.focused ? ', has audio' : ''}`
              : `Tile ${tile.index + 1}, empty`
          }
          onClick={() => onFocus(tile.index)}
          onDoubleClick={() => onPromote(tile.index)}
          style={{
            position: 'absolute',
            left: tile.rect.x / dpr,
            top: tile.rect.y / dpr,
            width: tile.rect.width / dpr,
            height: tile.rect.height / dpr,
            pointerEvents: 'auto',
            // See-through only once there is something to see through to.
            //
            // The window is transparent while a mosaic is open -- that is how the tiles
            // show at all -- so a tile that paints nothing is not "empty", it is a hole
            // straight to the desktop. A tile that is still opening its stream used to
            // be exactly that, and on a line that allows one connection three of the
            // four tiles stay that way: most of the window was the desktop showing
            // through. `status` is what tells them apart, and anything short of a
            // picture gets an opaque black screen to wait on.
            background:
              tile.error || tile.channelId === null
                ? 'color-mix(in srgb, var(--bg) 92%, transparent)'
                : tile.status === 'playing' || tile.status === 'paused'
                  ? 'transparent'
                  : '#000',
            border: tile.focused
              ? '2px solid var(--accent)'
              : '1px solid color-mix(in srgb, var(--text) 22%, transparent)',
            borderRadius: 0,
            padding: 0,
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'flex-end',
            alignItems: 'stretch',
            cursor: 'pointer',
            color: 'var(--text)',
            textAlign: 'left',
            overflow: 'hidden',
          }}
        >
          {tile.error && (
            <div
              style={{
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 'var(--sp-2)',
                padding: 'var(--sp-3)',
              }}
            >
              <Icon name="alert" size={20} />
              <span style={{ fontSize: 12, opacity: 0.85, textAlign: 'center' }}>
                {tile.error}
              </span>
            </div>
          )}

          {!tile.error && tile.channelId === null && (
            <div
              style={{
                flex: 1,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 12,
                opacity: 0.6,
              }}
            >
              Empty
            </div>
          )}

          {/* The number is the keyboard shortcut, so it is on screen rather than in a
              help panel: `1`–`9` move audio, which nobody discovers otherwise. */}
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 'var(--sp-2)',
              padding: '4px 8px',
              background: 'color-mix(in srgb, var(--bg) 70%, transparent)',
              fontSize: 12,
            }}
          >
            <span
              style={{
                fontVariantNumeric: 'tabular-nums',
                fontWeight: 600,
                opacity: tile.index < 9 ? 0.9 : 0.4,
              }}
            >
              {tile.index + 1}
            </span>
            <span
              style={{
                flex: 1,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {tile.name ?? '—'}
            </span>
            {tile.focused && <Icon name="volume" size={14} />}
          </div>
        </button>
      ))}

      <div
        style={{
          position: 'absolute',
          top: 'var(--sp-4)',
          right: 'var(--sp-4)',
          display: 'flex',
          gap: 'var(--sp-2)',
          pointerEvents: 'auto',
        }}
      >
        <Button icon="plus" onClick={onSave} data-testid="mosaic-save">
          Save layout
        </Button>
        <Button icon="close" onClick={onClose} data-testid="mosaic-close">
          Close
        </Button>
      </div>
    </div>
  );
}
