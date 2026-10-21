# Frontend Linting

The Community frontend uses ESLint for JavaScript and TypeScript, and
Stylelint for CSS and Less. `yarn lint` runs both checks and rejects warnings.

## Source Rules

- Maintained source must pass with zero errors and zero warnings.
- Do not disable or downgrade rules to make CI pass. Fix the source or update a
  rule only when the project runtime or syntax contract has changed.
- Intentionally unused callback parameters must start with `_`.
- CSS module classes may use camelCase or kebab-case. Global third-party class
  names may also use PascalCase.

## Generated Files

The following files are Alibaba Iconfont exports containing minified generated
runtime code. They are excluded by exact path and must be refreshed from their
generator rather than edited manually:

- `src/assets/fonts/new-chat2db-colourful-iconfont.js`
- `src/assets/fonts/new-chat2db-iconfont.js`

No other source path is exempt from ESLint or Stylelint.
