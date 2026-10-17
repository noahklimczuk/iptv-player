/**
 * The linter the code was already written for.
 *
 * Five files carried `eslint-disable-next-line` comments — one `no-console`, four
 * `react-hooks/exhaustive-deps` — for rules that nothing installed evaluated. There was
 * no `eslint` dependency, no config of any flavour, no `lint` script and no lint step in
 * CI; the only check on the TypeScript was `tsc --noEmit`. The suppressions were
 * evidence that a linter had been expected and never arrived (R-155).
 *
 * Each of those four `exhaustive-deps` suppressions carries a reason that holds, so the
 * value here is not in them. It is in `jsx-a11y`: the repeated problems this codebase
 * has had are the accessibility ones — dialogs that did not trap focus, a pause that
 * only existed on hover, a highlight no screen reader could follow, a tab target with no
 * key handler — and those are exactly the rules a linter can see and a type checker
 * cannot.
 *
 * `tsc --strict` stays the stronger half of the job and runs beside this.
 */
import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import jsxA11y from 'eslint-plugin-jsx-a11y';

/**
 * The handlers that are an interaction somebody could be locked out of.
 *
 * Deliberately narrower than the a11y rules' own default list, which also counts
 * `onMouseEnter`, `onMouseLeave` and `onError`. Those are a hover affordance and an
 * image that failed to load: the first always has a focus twin in this codebase and the
 * second is the artwork fallback on every poster in the app. What the rules are here for
 * is a `div` that behaves like a button.
 */
const INTERACTIONS = [
  'onClick', 'onMouseDown', 'onMouseUp',
  'onKeyPress', 'onKeyDown', 'onKeyUp',
];

export default tseslint.config(
  {
    // Nothing generated, nothing vendored, and not this file's own tooling.
    ignores: ['dist/**', 'src-native/**', 'node_modules/**', 'screenshots/**', 'test-results/**', 'playwright-report/**'],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      globals: { ...globals.browser, ...globals.es2022 },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: {
      'react-hooks': reactHooks,
      'jsx-a11y': jsxA11y,
    },
    rules: {
      // The two hooks rules this codebase was written against. Not the plugin's whole
      // recommended set: version 7 ships the React Compiler's rules as well --
      // `set-state-in-effect`, `refs`, `purity`, `incompatible-library` -- which flag
      // 36 places in working code and describe a migration to the compiler rather than
      // a defect in any of them. That is a decision to take on purpose, with the
      // compiler in hand, and not a side effect of installing a linter.
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      ...jsxA11y.flatConfigs.recommended.rules,

      // The app is a TV interface driven by a remote, and `tsc --strict` already has
      // the type half of this. What is left is the a11y half, so these are the rules
      // that earn their place:
      'jsx-a11y/click-events-have-key-events': 'error',
      'jsx-a11y/no-static-element-interactions': ['error', { handlers: INTERACTIONS }],
      // Only the handlers that are an interaction. The rule's default list also counts
      // `onMouseEnter`, `onMouseLeave` and `onError`, which are a hover affordance and
      // an image that failed to load -- neither is something a keyboard user is being
      // kept out of, and `<img onError>` is the artwork fallback on every poster in the
      // app. What the rule is here for is a div that behaves like a button.
      'jsx-a11y/no-noninteractive-element-interactions': [
        'error',
        { handlers: INTERACTIONS },
      ],
      // Off, with a reason. The app is driven by a remote and has no pointer: when a
      // dialog or a search field opens, something has to take focus or the viewer has
      // no way to type into it at all. The three places that use it are exactly those
      // -- the palette's input, the mosaic's picker, the playlist's filter -- and the
      // usability argument the rule is making is about a web page the reader arrived at
      // with a mouse and a tab key.
      'jsx-a11y/no-autofocus': 'off',
      'jsx-a11y/aria-role': 'error',
      'jsx-a11y/role-has-required-aria-props': 'error',

      // Deliberate throughout: the UI is inline-styled and the host is reached through
      // one typed `invoke`, so these two would be noise rather than findings.
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-empty-function': 'off',
      // `catch {}` where not knowing is the answer, which this codebase says in words at
      // every one of those sites.
      'no-empty': ['error', { allowEmptyCatch: true }],
      // An unused argument is often the shape of a callback. An unused *variable* is a
      // mistake either way.
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      // `console.warn` and `console.error` are how the UI reports what it cannot show.
      'no-console': ['error', { allow: ['warn', 'error'] }],
    },
  },
  {
    // Tests say what they are doing out loud, and a journey logs to be read.
    files: ['tests-e2e/**/*.ts', '**/*.test.ts', '**/*.test.tsx'],
    languageOptions: { globals: { ...globals.node } },
    rules: {
      'no-console': 'off',
      '@typescript-eslint/no-unused-expressions': 'off',
    },
  },
  {
    // Vite and Playwright config run in Node.
    files: ['*.config.ts', '*.config.js'],
    languageOptions: { globals: { ...globals.node } },
    rules: { 'no-console': 'off' },
  },
  {
    // Build scripts are Node programs that talk to whoever ran them.
    files: ['scripts/**/*.{js,mjs,cjs,ts}'],
    languageOptions: { globals: { ...globals.node } },
    rules: { 'no-console': 'off' },
  },
);
