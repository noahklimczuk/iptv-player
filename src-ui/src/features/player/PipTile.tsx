/**
 * The frame around the small picture (README §6.2, `P`).
 *
 * There is no video in here: the host moves the main player's own surface into a corner
 * and reports where, and this draws a label and three controls exactly over it. The
 * shell is clipped around that rectangle (`SurfaceHole`) so the picture shows through;
 * this frame is portalled to `document.body` to sit outside the clip.
 */
import './player.css';
import { createPortal } from 'react-dom';
import type { PipView } from '@shared/ipc';
import { Icon } from '@/components/Icon';

export function PipTile({
  view, title, dpr, onExpand, onClose, onCycleCorner,
}: {
  view: PipView;
  title: string | null;
  /** Device pixels per CSS pixel; the host's rectangle is in device pixels. */
  dpr: number;
  onExpand: () => void;
  onClose: () => void;
  onCycleCorner: () => void;
}) {
  if (!view.enabled || view.rect.width <= 0 || view.rect.height <= 0) return null;
  const scale = dpr > 0 ? dpr : 1;
  return createPortal(
    <div
      data-testid="pip-tile"
      data-corner={view.corner}
      className="pl-pip"
      role="group"
      aria-label={`Small picture: ${title ?? 'playing'}`}
      style={{
        left: view.rect.x / scale,
        top: view.rect.y / scale,
        width: view.rect.width / scale,
        height: view.rect.height / scale,
      }}
    >
      <div className="pl-pip-bar">
        <span className="pl-pip-title">{title ?? 'Playing'}</span>
        <button type="button" className="pl-pip-btn" data-testid="pip-corner"
          aria-label="Move the small picture to the next corner" title="Next corner" onClick={onCycleCorner}>
          <Icon name="layers" size={13} />
        </button>
        <button type="button" className="pl-pip-btn" data-testid="pip-expand"
          aria-label="Expand the small picture" title="Expand" onClick={onExpand}>
          <Icon name="fullscreen" size={13} />
        </button>
        <button type="button" className="pl-pip-btn" data-testid="pip-close"
          aria-label="Close the small picture" title="Close" onClick={onClose}>
          <Icon name="close" size={13} />
        </button>
      </div>
    </div>,
    document.body,
  );
}
