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
import { useEffect, useMemo, useState } from 'react';
import type { MosaicView, Programme } from '@shared/ipc';
import { invoke } from '@/ipc';
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

/**
 * Now and next for every channel on the wall, for the tiles that have no picture.
 *
 * A rotating mosaic shows one stream and eight placeholders, and a placeholder with
 * nothing but a channel name on it is a worse answer than the refusal it replaced: the
 * viewer picked those channels to know what is on them. One batched request for the
 * whole wall, refreshed on the quarter hour's worth of drift rather than on a timer,
 * because programmes do not change faster than that.
 *
 * Only asked for a rotating mosaic. An ordinary one has a picture in every tile and
 * nothing to fill in.
 */
function useWallGuide(channelIds: number[], enabled: boolean) {
  type Slice = Record<number, { now: Programme | null; next: Programme | null }>;
  const [guide, setGuide] = useState<Slice>({});
  const key = channelIds.join(',');
  useEffect(() => {
    if (!enabled || channelIds.length === 0) {
      setGuide({});
      return;
    }
    let live = true;
    const ask = () => {
      invoke('epg.nowNextMany', { channelIds })
        .then((reply: Slice) => {
          if (live) setGuide(reply);
        })
        .catch(() => {
          // A wall without programme names is still a usable wall. Nothing is said
          // about it: the tiles fall back to the channel name, which is what they had.
        });
    };
    ask();
    const timer = window.setInterval(ask, 60_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
    // `key` rather than the array: a new array with the same ids is the common case,
    // and re-asking on every render of the parent would be a request a second.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled]);
  return guide;
}

/**
 * The host's countdown, ticking between updates.
 *
 * `nextInSecs` arrives with each `mosaic.state` event, which is as often as the
 * heartbeat reports a change rather than once a second. Shown without this, the number
 * sat still and then jumped, which reads as broken rather than as a countdown.
 * Re-seeded from the host every time it sends one, so this never drifts away from the
 * clock that is actually deciding.
 */
function useCountdown(from: number | null): number | null {
  const [left, setLeft] = useState(from);
  useEffect(() => setLeft(from), [from]);
  useEffect(() => {
    if (from === null) return;
    const timer = window.setInterval(
      () => setLeft((n) => (n === null ? null : Math.max(0, n - 1))),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [from]);
  return from === null ? null : left;
}

export function MosaicOverlay({
  view,
  onFocus,
  onPromote,
  onClose,
  onSave,
  onResumeRotation,
}: {
  view: MosaicView;
  onFocus: (index: number) => void;
  onPromote: (index: number) => void;
  onClose: () => void;
  onSave: () => void;
  /** Let a rotation the viewer pinned by clicking a tile start moving again. */
  onResumeRotation?: () => void;
}) {
  const dpr = useDevicePixelRatio();
  const wall = useMemo(
    () => view.tiles.map((t) => t.channelId).filter((id): id is number => id !== null),
    [view.tiles],
  );
  const guide = useWallGuide(wall, view.rotating);
  const countdown = useCountdown(view.nextInSecs);

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
              ? `Tile ${tile.index + 1}: ${tile.name}${
                  view.rotating && !tile.showing ? ', waiting its turn' : ''
                }${tile.focused ? ', has audio' : ''}`
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
                ? 'var(--surface-sunk)'
                : view.rotating && !tile.showing
                  // Waiting its turn. Opaque for the reason above -- the window is
                  // transparent while a mosaic is open -- and a shade off the player's
                  // ink so the wall reads as eight cards and one picture rather than as
                  // one picture and eight failures.
                  ? 'color-mix(in srgb, var(--player-bg) 94%, var(--on-art))'
                  : tile.status === 'playing' || tile.status === 'paused'
                    ? 'transparent'
                    : 'var(--player-bg)',
            // Square corners: the picture under a tile is the host's rectangle, and a
            // rounded tile would leave its corners showing past the frame. The sound tile
            // carries Daylight's 4px blue ring, drawn inside so it never covers a
            // neighbour.
            border: tile.channelId === null && !tile.error
              ? '2px dashed var(--border-strong)'
              : 'none',
            boxShadow: tile.focused ? 'inset 0 0 0 4px var(--accent)' : 'none',
            borderRadius: 0,
            padding: 0,
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'flex-end',
            alignItems: 'stretch',
            cursor: 'pointer',
            color: tile.channelId === null && !tile.error ? 'var(--text)' : 'var(--on-art)',
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

          {/*
            * A tile waiting its turn. The channel it is standing for, what is on it now
            * and what is next -- which is the whole reason the viewer picked it.
            */}
          {!tile.error && tile.channelId !== null && view.rotating && !tile.showing && (
            <div
              style={{
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'flex-start',
                justifyContent: 'center',
                gap: 4,
                padding: 'var(--sp-3)',
                minWidth: 0,
              }}
            >
              <span
                style={{
                  fontSize: 13,
                  fontWeight: 600,
                  maxWidth: '100%',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {tile.name}
              </span>
              {guide[tile.channelId]?.now && (
                <span
                  style={{
                    fontSize: 12,
                    opacity: 0.85,
                    maxWidth: '100%',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {guide[tile.channelId]!.now!.title}
                </span>
              )}
              {guide[tile.channelId]?.next && (
                <span
                  style={{
                    fontSize: 11,
                    opacity: 0.55,
                    maxWidth: '100%',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  Next: {guide[tile.channelId]!.next!.title}
                </span>
              )}
            </div>
          )}

          {!tile.error && tile.channelId === null && (
            <div
              style={{
                flex: 1,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 'var(--fs-sm)',
                fontWeight: 700,
                color: 'var(--text-muted)',
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
              padding: '18px 14px 10px',
              background: tile.channelId === null && !tile.error
                ? 'transparent'
                : 'linear-gradient(to top, rgb(10 12 20 / 0.75), transparent)',
              fontSize: 'var(--fs-sm)',
              fontWeight: 600,
            }}
          >
            <span
              style={{
                fontVariantNumeric: 'tabular-nums',
                fontWeight: 800,
                opacity: tile.index < 9 ? 1 : 0.4,
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
          alignItems: 'center',
          gap: 'var(--sp-2)',
          padding: 6,
          borderRadius: 'var(--r-lg)',
          background: 'var(--dock-bg)',
          color: 'var(--dock-text)',
          boxShadow: 'var(--shadow-dock)',
          pointerEvents: 'auto',
        }}
      >
        <span
          className="dl-display"
          style={{ fontSize: 'var(--fs-xl)', padding: '0 10px 0 8px' }}
        >
          Multi-view
        </span>
        {view.rotating && (
          <div
            data-testid="mosaic-rotation-status"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 'var(--sp-2)',
              padding: '0 var(--sp-3)',
              height: 36,
              borderRadius: 'var(--r-md)',
              background: 'var(--dock-btn)',
              fontSize: 'var(--fs-xs)',
              fontWeight: 700,
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            <Icon name="layers" size={14} />
            {countdown === null ? (
              <>
                <span>Staying on {view.tiles[view.focused]?.name ?? 'this tile'}</span>
                {onResumeRotation && (
                  <Button
                    size="sm"
                    data-testid="mosaic-resume-rotation"
                    onClick={onResumeRotation}
                  >
                    Resume
                  </Button>
                )}
              </>
            ) : (
              <span>Next tile in {countdown}s</span>
            )}
          </div>
        )}
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
