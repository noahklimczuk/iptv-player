import clsx from 'clsx';
import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from 'react';
import { Icon, type IconName } from './Icon';

/* ── Button ───────────────────────────────────────────────────────────────── */

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
type Size = 'sm' | 'md' | 'lg';

const SIZES: Record<Size, CSSProperties> = {
  sm: { height: 32, padding: '0 12px', fontSize: 'var(--fs-sm)', gap: 6 },
  md: { height: 40, padding: '0 18px', fontSize: 'var(--fs-md)', gap: 8 },
  lg: { height: 50, padding: '0 28px', fontSize: 'var(--fs-lg)', gap: 10 },
};

export function Button({
  variant = 'secondary', size = 'md', icon, iconFilled, children, style, ...rest
}: {
  variant?: Variant;
  size?: Size;
  icon?: IconName;
  iconFilled?: boolean;
  children?: ReactNode;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  const base: CSSProperties = {
    ...SIZES[size],
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    border: '1px solid transparent', borderRadius: 'var(--r-md)',
    fontWeight: 600, whiteSpace: 'nowrap',
    // A disabled control must not look clickable.
    cursor: rest.disabled ? 'not-allowed' : 'pointer',
    opacity: rest.disabled ? 0.45 : 1,
    transition: `background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease),
                 border-color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease)`,
  };
  const variants: Record<Variant, CSSProperties> = {
    primary: { background: 'var(--text)', color: 'var(--text-invert)' },
    secondary: {
      background: 'color-mix(in srgb, var(--surface) 78%, transparent)',
      color: 'var(--text)', borderColor: 'var(--border)',
      backdropFilter: 'blur(12px)',
    },
    ghost: { background: 'transparent', color: 'var(--text-muted)' },
    danger: { background: 'var(--danger)', color: '#fff' },
  };
  return (
    <button
      {...rest}
      className={clsx('aurora-btn', rest.className)}
      style={{ ...base, ...variants[variant], ...style }}
    >
      {icon && <Icon name={icon} size={size === 'lg' ? 22 : 18} filled={iconFilled} />}
      {children}
    </button>
  );
}

/** Circular icon-only button, used in the OSD and on card hover. */
export function IconButton({
  icon, label, active, size = 38, filled, ...rest
}: {
  icon: IconName; label: string; active?: boolean; size?: number; filled?: boolean;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...rest}
      aria-label={label}
      title={label}
      style={{
        width: size, height: size, display: 'grid', placeItems: 'center',
        borderRadius: 'var(--r-full)',
        border: `1px solid ${active ? 'var(--text)' : 'var(--border-strong)'}`,
        background: active
          ? 'var(--text)'
          : 'color-mix(in srgb, var(--surface) 70%, transparent)',
        color: active ? 'var(--text-invert)' : 'var(--text)',
        cursor: 'pointer', backdropFilter: 'blur(12px)',
        transition: 'all var(--t-fast) var(--ease)',
        ...rest.style,
      }}
    >
      <Icon name={icon} size={Math.round(size * 0.48)} filled={filled} />
    </button>
  );
}

/* ── Badge ────────────────────────────────────────────────────────────────── */

export function Badge({
  children, tone = 'neutral', style,
}: {
  children: ReactNode;
  tone?: 'neutral' | 'accent' | 'live' | 'new' | 'outline';
  style?: CSSProperties;
}) {
  const tones: Record<string, CSSProperties> = {
    neutral: { background: 'var(--surface)', color: 'var(--text-muted)' },
    accent: { background: 'var(--accent)', color: 'var(--accent-text)' },
    live: { background: 'var(--live)', color: '#fff' },
    new: { background: 'var(--success)', color: '#04140d' },
    outline: {
      background: 'transparent', color: 'var(--text-muted)',
      border: '1px solid var(--border-strong)',
    },
  };
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 4,
        padding: '2px 7px', borderRadius: 'var(--r-sm)',
        fontSize: 'var(--fs-xs)', fontWeight: 700, letterSpacing: '0.03em',
        lineHeight: 1.5, textTransform: 'uppercase',
        ...tones[tone], ...style,
      }}
    >
      {children}
    </span>
  );
}

/* ── Progress bar ─────────────────────────────────────────────────────────── */

export function ProgressBar({ percent, height = 3 }: { percent: number; height?: number }) {
  return (
    <div
      style={{
        height, width: '100%', background: 'color-mix(in srgb, var(--text) 22%, transparent)',
        borderRadius: 'var(--r-full)', overflow: 'hidden',
      }}
      role="progressbar"
      aria-valuenow={Math.round(percent)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div
        style={{
          height: '100%', width: `${Math.max(0, Math.min(100, percent))}%`,
          background: 'var(--accent)', borderRadius: 'var(--r-full)',
          transition: 'width var(--t-base) var(--ease)',
        }}
      />
    </div>
  );
}

/* ── Skeletons & empty states ─────────────────────────────────────────────── */

export function Skeleton({ w, h, r = 8 }: { w?: number | string; h: number; r?: number }) {
  return <div className="skeleton" style={{ width: w ?? '100%', height: h, borderRadius: r }} />;
}

export function EmptyState({
  icon = 'sparkle', title, body, action,
}: { icon?: IconName; title: string; body?: string; action?: ReactNode }) {
  return (
    <div
      style={{
        display: 'grid', placeItems: 'center', gap: 'var(--sp-3)',
        padding: 'var(--sp-8) var(--sp-4)', textAlign: 'center', color: 'var(--text-muted)',
      }}
    >
      <div
        style={{
          width: 56, height: 56, borderRadius: 'var(--r-full)', display: 'grid',
          placeItems: 'center', background: 'var(--surface)', color: 'var(--accent)',
        }}
      >
        <Icon name={icon} size={26} />
      </div>
      <div style={{ fontSize: 'var(--fs-lg)', fontWeight: 650, color: 'var(--text)' }}>
        {title}
      </div>
      {body && <div style={{ maxWidth: 440, fontSize: 'var(--fs-sm)' }}>{body}</div>}
      {action}
    </div>
  );
}

/** Artwork with a coloured placeholder and fade-in — no layout shift (README §12). */
export function Poster({
  src, alt, radius = 'var(--r-md)', ratio = 2 / 3,
}: { src: string | null; alt: string; radius?: string; ratio?: number }) {
  return (
    <div
      style={{
        position: 'relative', width: '100%', aspectRatio: String(ratio),
        background: 'var(--surface)', borderRadius: radius, overflow: 'hidden',
      }}
    >
      {src ? (
        <img
          src={src} alt={alt} loading="lazy" decoding="async"
          style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }}
        />
      ) : (
        <div
          style={{
            display: 'grid', placeItems: 'center', height: '100%',
            color: 'var(--text-faint)', fontSize: 'var(--fs-2xl)', fontWeight: 800,
          }}
        >
          {alt.slice(0, 1).toUpperCase()}
        </div>
      )}
    </div>
  );
}
