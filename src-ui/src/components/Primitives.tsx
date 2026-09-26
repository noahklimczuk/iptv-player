import clsx from 'clsx';
import { useEffect, useMemo, useRef, useState } from 'react';
import type {
  ButtonHTMLAttributes, CSSProperties, InputHTMLAttributes, ReactNode,
} from 'react';
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
      // So the stylesheet can tell a filled button from a ghost one: one brightens,
      // the other has to grow a background before there is anything to brighten.
      data-variant={variant}
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

/* ── Text input ───────────────────────────────────────────────────────────── */

/**
 * The one text field.
 *
 * There were sixteen raw `<input>`s across eight screens, each carrying its own copy
 * of the same six style properties — so they were the same by coincidence rather than
 * by construction, and three of them had already drifted. A field is not a hard thing
 * to style; it is a hard thing to style *identically* sixteen times.
 */
/**
 * The shape of every text field in the app.
 *
 * Exported because a handful of screens build their own `<input>` for layout reasons
 * — a field that has to flex inside a row, or fill a form cell — and those still have
 * to be the same field. There were four copies of this object and they had all
 * drifted: 6px, 7px, 8px and 11px of padding, two different radii, two different
 * borders and two different backgrounds, on controls sitting next to each other.
 *
 * Use it with `className="aurora-field"`, which is where the hover and focus states
 * live.
 */
export const FIELD: CSSProperties = {
  padding: '7px 10px',
  background: 'var(--surface)',
  color: 'var(--text)',
  border: '1px solid var(--border-strong)',
  borderRadius: 'var(--r-md)',
  fontSize: 'var(--fs-sm)',
  fontFamily: 'inherit',
  transition: 'border-color var(--t-fast) var(--ease)',
};

export function TextField({
  icon, clearable, onClear, style, ...rest
}: {
  /** A glyph inside the field, on the left. */
  icon?: IconName;
  /** Show a clear button while there is something to clear. */
  clearable?: boolean;
  onClear?: () => void;
} & InputHTMLAttributes<HTMLInputElement>) {
  const hasValue = Boolean(rest.value ?? rest.defaultValue);
  return (
    <div style={{ position: 'relative', display: 'inline-flex', alignItems: 'center' }}>
      {icon && (
        <Icon
          name={icon} size={15}
          style={{
            position: 'absolute', left: 10, color: 'var(--text-faint)',
            pointerEvents: 'none',
          }}
        />
      )}
      <input
        {...rest}
        className={clsx('aurora-field', rest.className)}
        style={{
          ...FIELD,
          width: '100%',
          padding: `7px ${clearable && hasValue ? 28 : 10}px 7px ${icon ? 30 : 10}px`,
          ...style,
        }}
      />
      {clearable && hasValue && (
        <button
          type="button"
          aria-label="Clear"
          onClick={onClear}
          style={{
            position: 'absolute', right: 6, display: 'grid', placeItems: 'center',
            background: 'none', border: 'none', cursor: 'pointer',
            color: 'var(--text-faint)', padding: 4,
          }}
        >
          <Icon name="close" size={13} />
        </button>
      )}
    </div>
  );
}

/* ── Select ───────────────────────────────────────────────────────────────── */

export interface SelectOption {
  value: string;
  label: string;
  /** Shown right-aligned and dimmed — a count, usually. */
  hint?: string;
}

/**
 * Choose one of a list. Replaces every native `<select>` in the app.
 *
 * Not a style preference. A native `<select>` renders as a **blank rectangle** in the
 * WebView Aurora ships inside — which is why the old genre filter, the season picker
 * and the playlist filters all appeared as empty boxes in every screenshot, and why
 * nobody could tell what any of them were set to. Four screens had one.
 *
 * It also has to cope with lists a native control never could: a real subscription
 * publishes two hundred categories, which is not something anybody scrolls. So the
 * popover has its own filter once the list is long enough to need one.
 */
export function Select({
  options, value, onChange, placeholder, label, searchAfter = 12, width = 280, style,
}: {
  options: SelectOption[];
  /** `undefined` means nothing is chosen, and `placeholder` shows. */
  value?: string;
  onChange: (value: string | undefined) => void;
  /** What the "nothing chosen" row says. Omit to make a choice compulsory. */
  placeholder?: string;
  /** The accessible name. */
  label: string;
  /** Offer a filter box once there are at least this many options. */
  searchAfter?: number;
  width?: number;
  style?: CSSProperties;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState('');
  const box = useRef<HTMLDivElement | null>(null);

  // Click anywhere else and it closes, which is the one thing every popover must do
  // and the one thing hand-rolled ones forget.
  useEffect(() => {
    if (!open) return undefined;
    const away = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('mousedown', away);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('mousedown', away);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle
      ? options.filter((o) => o.label.toLowerCase().includes(needle))
      : options;
  }, [options, filter]);

  const current = options.find((o) => o.value === value);

  return (
    <div ref={box} style={{ position: 'relative', ...style }}>
      <Button
        size="sm"
        variant={current ? 'primary' : 'secondary'}
        icon="chevronDown"
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="listbox"
        data-testid={`select-${label.toLowerCase().replace(/\s+/g, '-')}`}
        onClick={() => { setOpen((o) => !o); setFilter(''); }}
      >
        <span
          style={{
            maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis',
            display: 'inline-block', verticalAlign: 'bottom',
          }}
        >
          {current?.label ?? placeholder ?? label}
        </span>
      </Button>

      {open && (
        <div
          role="listbox"
          aria-label={label}
          style={{
            position: 'absolute', top: 'calc(100% + 6px)', left: 0, zIndex: 60,
            width, maxHeight: 340, overflowY: 'auto',
            background: 'var(--bg-elevated)',
            border: '1px solid var(--border-strong)',
            borderRadius: 'var(--r-md)', boxShadow: 'var(--shadow-3)',
            padding: 'var(--sp-2)',
          }}
        >
          {options.length >= searchAfter && (
            <TextField
              autoFocus
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder={`Filter ${label.toLowerCase()}…`}
              aria-label={`Filter ${label.toLowerCase()}`}
              style={{ marginBottom: 'var(--sp-2)', width: '100%' }}
            />
          )}
          {placeholder !== undefined && (
            <SelectRow
              active={value === undefined}
              onClick={() => { onChange(undefined); setOpen(false); }}
            >
              {placeholder}
            </SelectRow>
          )}
          {shown.map((o) => (
            <SelectRow
              key={o.value}
              active={o.value === value}
              onClick={() => { onChange(o.value); setOpen(false); }}
            >
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{o.label}</span>
              {o.hint && (
                <span
                  style={{
                    marginLeft: 'auto', paddingLeft: 10, opacity: 0.55,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {o.hint}
                </span>
              )}
            </SelectRow>
          ))}
          {shown.length === 0 && (
            <div
              style={{
                padding: 'var(--sp-3)', color: 'var(--text-faint)',
                fontSize: 'var(--fs-sm)',
              }}
            >
              Nothing matches that.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SelectRow({
  active, onClick, children,
}: {
  active: boolean; onClick: () => void; children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={active}
      onClick={onClick}
      data-testid="select-row"
      style={{
        display: 'flex', alignItems: 'center', width: '100%', textAlign: 'left',
        padding: '7px 9px', borderRadius: 'var(--r-sm)', border: 'none',
        cursor: 'pointer', fontSize: 'var(--fs-sm)', whiteSpace: 'nowrap',
        background: active ? 'var(--surface-hover)' : 'transparent',
        color: active ? 'var(--text)' : 'var(--text-muted)',
        fontWeight: active ? 700 : 500,
        transition: 'background var(--t-fast) var(--ease)',
      }}
      onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--surface)'; }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = active ? 'var(--surface-hover)' : 'transparent';
      }}
    >
      {children}
    </button>
  );
}
