import clsx from 'clsx';
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import type {
  ButtonHTMLAttributes, CSSProperties, InputHTMLAttributes,
  KeyboardEvent as ReactKeyboardEvent, ReactNode,
} from 'react';
import { fallBackToRemote, useAssetSrc } from '@/hooks/useAssetSrc';

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
        // The same treatment `Button` above gives it. This hardcoded `pointer` and full
        // opacity, so a disabled icon button looked and felt enabled while doing
        // nothing. No caller passes `disabled` today, which is why it was never seen --
        // and why the next one to need it would not have thought to check (R-116).
        cursor: rest.disabled ? 'not-allowed' : 'pointer',
        opacity: rest.disabled ? 0.45 : 1,
        backdropFilter: 'blur(12px)',
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

/**
 * A bar, named if it is telling the viewer something nothing else on screen is.
 *
 * It used to declare `role="progressbar"` with `aria-valuenow` and no accessible name at
 * all, so a screen reader announced "44 percent" of nothing. Both answers are here
 * because both are needed: beside a line of text that already reads "128 of 512" the bar
 * is decoration and `aria-hidden` is the honest markup, and where it is the only
 * indication it needs a name (R-116).
 *
 * So `label` being absent is a statement that something adjacent already says this,
 * rather than an oversight.
 */
export function ProgressBar({
  percent, height = 3, label,
}: {
  percent: number;
  height?: number;
  /** What is progressing. Omit only when adjacent text already says it. */
  label?: string;
}) {
  return (
    <div
      style={{
        height, width: '100%', background: 'color-mix(in srgb, var(--text) 22%, transparent)',
        borderRadius: 'var(--r-full)', overflow: 'hidden',
      }}
      {...(label
        ? {
            role: 'progressbar',
            'aria-label': label,
            'aria-valuenow': Math.round(percent),
            'aria-valuemin': 0,
            'aria-valuemax': 100,
          }
        : { 'aria-hidden': true })}
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
  // From disk when the cache holds it, from the provider when it does not.
  const resolved = useAssetSrc(src);
  return (
    <div
      style={{
        position: 'relative', width: '100%', aspectRatio: String(ratio),
        background: 'var(--surface)', borderRadius: radius, overflow: 'hidden',
      }}
    >
      {resolved ? (
        <img
          src={resolved} alt={alt} loading="lazy" decoding="async"
          onError={(e) => fallBackToRemote(e, src)}
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
  /**
   * A heading to file this option under.
   *
   * Options keep the order they are given in; this only draws a heading when the group
   * changes, so the caller decides both the grouping and the order within it. Added for
   * multi-view's channel picker, where a flat list of several thousand names could only
   * be searched and not browsed — you had to know what you were looking for before you
   * could look for it.
   */
  group?: string;
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
  /** Which row the keyboard is on. `aria-activedescendant` points at it. */
  const [cursor, setCursor] = useState(0);
  const box = useRef<HTMLDivElement | null>(null);
  const list = useRef<HTMLDivElement | null>(null);
  /** Whether this has ever been open, so the first render does not grab the focus. */
  const wasOpen = useRef(false);
  const slug = label.toLowerCase().replace(/\s+/g, '-');
  const optionId = (index: number) => `select-${slug}-option-${index}`;

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
      ? options.filter(
          (o) =>
            o.label.toLowerCase().includes(needle) ||
            // The group and the hint as well: on a real library the useful search is
            // "sports" or a channel number, and neither is in the name.
            (o.group ?? '').toLowerCase().includes(needle) ||
            (o.hint ?? '').toLowerCase().includes(needle),
        )
      : options;
  }, [options, filter]);

  const current = options.find((o) => o.value === value);
  /** Whether the filter box is drawn, which decides where the focus goes on open. */
  const filtering = options.length >= searchAfter;

  /**
   * The rows the keyboard can land on, in the order they are drawn.
   *
   * The placeholder is one of them: it is a real choice -- "no filter", "any language" --
   * and a list whose first row the arrow keys skipped would be one where clearing the
   * selection is mouse-only.
   */
  const rows = useMemo<{ value: string | undefined }[]>(
    () =>
      placeholder !== undefined
        ? [{ value: undefined }, ...shown.map((o) => ({ value: o.value }))]
        : shown.map((o) => ({ value: o.value })),
    [placeholder, shown],
  );
  // Clamped at render rather than tracked: `rows` shrinks as the filter narrows, and a
  // cursor left past the end would point `aria-activedescendant` at an id nothing has.
  const at = Math.min(cursor, Math.max(0, rows.length - 1));

  /**
   * Where the keyboard starts when the list opens: on the current choice.
   *
   * Computed from `options` rather than from `rows`, and set from the two event handlers
   * rather than from an effect. An effect is what this was, keyed on `[open, rows,
   * value]` -- and every caller passes `options` as a fresh array literal, so `shown`
   * and `rows` were new objects on every render, the effect ran after every arrow press,
   * and it put the cursor straight back on the selected row. The arrow keys moved
   * nothing, which is the bug they were added to fix.
   */
  const startAt = () => {
    if (value === undefined) return 0;
    const found = options.findIndex((o) => o.value === value);
    if (found === -1) return 0;
    return placeholder !== undefined ? found + 1 : found;
  };

  /**
   * Focus into the popover on open, and back to the trigger on close.
   *
   * The same omission as R-102, in the control that replaced every native `<select>` in
   * the app, so it was on four screens at once. Opening left the focus on the trigger,
   * which is why the arrow keys below would have had nowhere to arrive; closing left it
   * nowhere at all, so Tab from a closed Select started again at the top of the page.
   *
   * The filter box takes it when there is one, because typing is the first thing to do in
   * a list long enough to need filtering. The listbox itself takes it otherwise, so
   * `aria-activedescendant` is on an element that actually has the focus.
   */
  useEffect(() => {
    if (open) {
      wasOpen.current = true;
      if (!filtering) list.current?.focus();
      return;
    }
    if (!wasOpen.current) return;
    wasOpen.current = false;
    // Only when the focus has nowhere better to be: closing because the viewer clicked
    // another control must not drag it back here.
    const active = document.activeElement;
    // The trigger is the first button inside the wrapper. Found that way rather than
    // held in a ref because `Button` is a plain function component, so a `ref` on it
    // would not reach the element -- and giving the shared primitive a `forwardRef` so
    // that one Select can restore focus would be shaping it around this call site.
    if (active == null || active === document.body) {
      box.current?.querySelector('button')?.focus();
    }
  }, [open, filtering]);

  const move = (to: number) => {
    if (rows.length === 0) return;
    const next = Math.max(0, Math.min(rows.length - 1, to));
    setCursor(next);
    // Kept on screen, which `maxHeight: 340` makes necessary -- the same thing R-147
    // found missing in the palette.
    document.getElementById(optionId(next))?.scrollIntoView({ block: 'nearest' });
  };

  /**
   * Up, Down, Home, End and Enter.
   *
   * Owed the moment this declared `listbox` and `option`: a screen-reader user told it is
   * a listbox presses Down, and nothing happened. Tab did work, because the rows are real
   * `<button>`s -- a legitimate pattern on its own, but not the one the roles announce,
   * and the roles are what gets believed (R-116).
   *
   * On the popover rather than on each row, so the keys work while the focus is in the
   * filter box: arrowing straight out of a half-typed filter is the point of having one.
   */
  const onKeys = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        move(at + 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        move(at - 1);
        break;
      case 'Home':
        e.preventDefault();
        move(0);
        break;
      case 'End':
        e.preventDefault();
        move(rows.length - 1);
        break;
      case 'Enter':
      case ' ': {
        // Space is a character in the filter box and a choice on the list. Only the
        // second is ours to take.
        if (e.key === ' ' && e.target !== list.current) return;
        const row = rows[at];
        if (!row) return;
        e.preventDefault();
        onChange(row.value);
        setOpen(false);
        break;
      }
      default:
        break;
    }
  };

  return (
    <div ref={box} style={{ position: 'relative', ...style }}>
      <Button
        size="sm"
        variant={current ? 'primary' : 'secondary'}
        icon="chevronDown"
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="listbox"
        data-testid={`select-${slug}`}
        onClick={() => { setOpen((o) => !o); setFilter(''); setCursor(startAt()); }}
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
          ref={list}
          role="listbox"
          aria-label={label}
          tabIndex={-1}
          aria-activedescendant={rows.length > 0 ? optionId(at) : undefined}
          onKeyDown={onKeys}
          style={{
            // No ring on the box itself: the highlighted row answers "where am I", and
            // an outline around the whole popover would compete with it.
            outline: 'none',
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
              onChange={(e) => { setFilter(e.target.value); setCursor(0); }}
              placeholder={`Filter ${label.toLowerCase()}…`}
              aria-label={`Filter ${label.toLowerCase()}`}
              style={{ marginBottom: 'var(--sp-2)', width: '100%' }}
            />
          )}
          {placeholder !== undefined && (
            <SelectRow
              id={optionId(0)}
              active={value === undefined}
              cursored={at === 0}
              onClick={() => { onChange(undefined); setOpen(false); }}
            >
              {placeholder}
            </SelectRow>
          )}
          {shown.map((o, i) => (
            <Fragment key={o.value}>
              {o.group && o.group !== shown[i - 1]?.group && (
                <div
                  data-testid="select-group"
                  // A listbox's children are options or groups, so a bare `div` in here
                  // is a stray node to anything reading the roles. It is a label drawn
                  // between rows, and this says so (R-116).
                  role="presentation"
                  // Sticky, because the whole point is knowing where you are in a list
                  // long enough to need scrolling.
                  style={{
                    position: 'sticky',
                    top: 0,
                    zIndex: 1,
                    background: 'var(--bg-elevated)',
                    padding: '6px var(--sp-2) 3px',
                    fontSize: 'var(--fs-xs)',
                    fontWeight: 700,
                    letterSpacing: '0.04em',
                    textTransform: 'uppercase',
                    color: 'var(--text-faint)',
                  }}
                >
                  {o.group}
                </div>
              )}
            <SelectRow
              id={optionId(placeholder !== undefined ? i + 1 : i)}
              active={o.value === value}
              cursored={at === (placeholder !== undefined ? i + 1 : i)}
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
            </Fragment>
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
  active, cursored, id, onClick, children,
}: {
  active: boolean;
  /** Where the keyboard is, which is not the same thing as what is chosen. */
  cursored: boolean;
  id: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="option"
      id={id}
      aria-selected={active}
      onClick={onClick}
      data-testid="select-row"
      data-cursored={cursored || undefined}
      style={{
        // A ring rather than a background, so "the keyboard is here" and "this is the
        // current choice" can be true of different rows and both stay visible.
        boxShadow: cursored ? 'inset 0 0 0 2px var(--accent)' : 'none',
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
