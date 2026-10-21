module.exports = {
  parser: '@typescript-eslint/parser',
  env: {
    browser: true,
    es2021: true,
  },
  plugins: ['@typescript-eslint', 'babel', 'react-hooks', 'react', 'unused-imports'],
  extends: [
    'eslint:recommended',
    'plugin:@typescript-eslint/recommended',
    'plugin:react/recommended',
    'plugin:react/jsx-runtime',
    // 'airbnb-base', // airbnb-base already includes eslint-plugin-import.
    // 'prettier', // Disable conflicting ESLint formatting rules in favor of Prettier.
    // 'prettier/@typescript-eslint', // Disable conflicting TypeScript formatting rules in favor of Prettier.
  ],
  overrides: [
    {
      env: {
        node: true,
      },
      files: ['.eslintrc.{js,cjs}'], //
      parserOptions: {
        sourceType: 'script',
      },
    },
  ],
  parserOptions: {
    ecmaVersion: 'latest',
    sourceType: 'module',
  },
  settings: {
    react: {
      version: 'detect',
    },
  },
  ignorePatterns: [
    // Alibaba Iconfont exports. Regenerate these files instead of editing the minified vendor runtime.
    'src/assets/fonts/new-chat2db-colourful-iconfont.js',
    'src/assets/fonts/new-chat2db-iconfont.js',
  ],
  rules: {
    'func-names': 0, // Allow anonymous function expressions.
    'one-var': [1, 'never'], // Warn unless variables use separate declarations.
    'prefer-const': 1, // Warn when a variable can be declared with const.
    'no-unused-expressions': 0, // Allow unused expressions.
    'new-cap': 2, // Require constructor names to start with a capital letter.
    'prefer-arrow-callback': 2, // Require arrow callbacks where applicable.
    'arrow-body-style': 0, // Do not enforce a specific arrow-function body style.
    'max-len': [
      // Warn when a line exceeds the configured maximum length.
      1,
      {
        code: 120,
        ignoreStrings: true,
        ignoreUrls: true,
        ignoreRegExpLiterals: true,
      },
    ],
    'consistent-return': 'off', // Allow functions to mix explicit and implicit returns.
    'default-case': 2, // Require a default case in switch statements.
    'prefer-rest-params': 2, // Require rest parameters instead of arguments.
    'no-script-url': 0, // Allow javascript: URLs.
    // 'no-console': [ // Disable the use of console
    //   2,
    //   {
    //     allow: ['info', 'error', 'warn'],
    //   },
    // ],
    'no-duplicate-imports': [2], // Disallow duplicate imports.
    'newline-per-chained-call': 2, // Require line breaks in long method chains.
    // 'no-underscore-dangle': 2, // Disallow dangling underscores in identifiers.
    'eol-last': 2, // Require a newline at the end of each file.
    'no-useless-rename': 2, // Disallow redundant import, export, and destructuring renames.
    'no-undef': 0, // Let TypeScript handle undefined-variable checks.
    'class-methods-use-this': 0, // Allow class methods that do not reference this.
    'prefer-destructuring': 0, // Do not require array or object destructuring.
    'no-unused-vars': 0, // Let the TypeScript-specific rule handle unused variables.
    '@typescript-eslint/no-unused-vars': 0,
    'unused-imports/no-unused-imports': 2,
    'unused-imports/no-unused-vars': [
      2,
      {
        args: 'after-used',
        argsIgnorePattern: '^_',
        caughtErrors: 'all',
        caughtErrorsIgnorePattern: '^_',
        destructuredArrayIgnorePattern: '^_',
        ignoreRestSiblings: true,
        vars: 'all',
        varsIgnorePattern: '^_',
      },
    ],
    'react/self-closing-comp': 2, // Require self-closing syntax for components without children.
    'react/jsx-indent-props': [2, 2], // Require two-space JSX prop indentation.
    'no-plusplus': 0, // Allow ++ and --.
    'react/jsx-uses-vars': 1, // Mark variables used in JSX as used.
    // 'react/no-multi-comp': [ // Prohibit multiple components from being defined in one file
    //   2,
    //   {
    //     ignoreStateless: true,
    //   },
    // ],
    'react/sort-comp': 1, // Warn about component method ordering.
    'react/jsx-tag-spacing': 2, // Enforce spacing around JSX tag delimiters.
    'react/jsx-no-bind': 0, // Allow bind and arrow functions in JSX props.
    'react/jsx-closing-bracket-location': 2, // Enforce JSX closing-bracket placement.
    'react/prefer-stateless-function': 0, // Do not require stateless components.
    'react/display-name': 0, // Do not require component displayName values.
    'react/prop-types': 0, // Do not require propTypes when TypeScript supplies types.
    'import/prefer-default-export': 0, // Do not require a default export for a single export.
    '@typescript-eslint/no-var-requires': 2, // Disallow require() imports.
    'no-use-before-define': 0, // Let the TypeScript-specific rule handle declaration order.
    '@typescript-eslint/no-use-before-define': [
      // Allow references before definitions.
      0,
      // {
      //   functions: false,
      // },
    ],
    '@typescript-eslint/explicit-function-return-type': 0, // Do not require explicit function return types.
    '@typescript-eslint/interface-name-prefix': 0, // Do not enforce an interface-name prefix.
    '@typescript-eslint/explicit-module-boundary-types': 0, // Do not require explicit exported API types.
    'no-shadow': 0, // Let the TypeScript-specific rule handle shadowing.
    '@typescript-eslint/no-shadow': 1, // Disallow shadowing outer-scope variables. TODO: consider severity 2.
    'no-invalid-this': 0, // Allow this outside class methods.
    'no-await-in-loop': 'off', // Allow await inside loops.
    'array-callback-return': 'off', // Allow array callbacks without an explicit return.
    'no-restricted-syntax': 'off', // Do not apply syntax restrictions.
    '@typescript-eslint/no-explicit-any': 0, // Allow explicit any types.
    'import/no-extraneous-dependencies': 0, // Do not enforce dependency declaration boundaries.
    'import/no-unresolved': 0, // Do not let ESLint resolve imports.
    '@typescript-eslint/explicit-member-accessibility': 0, // Do not require explicit member accessibility.
    '@typescript-eslint/no-object-literal-type-assertion': 0, // Allow object-literal type assertions.
    'react/no-find-dom-node': 0, // Allow findDOMNode.
    'no-param-reassign': [
      // Reassignment of function parameters is prohibited
      2,
      {
        props: false,
      },
    ],
    'arrow-parens': 0, // Do not enforce arrow-function parameter parentheses.
    indent: 0, // Do not enforce indentation with this rule.
    'operator-linebreak': [0], // Do not enforce operator line-break placement.
    'max-classes-per-file': [2, 10], // Allow at most ten classes per file.
    '@typescript-eslint/no-empty-function': [0], // Allow empty functions.
    'import/extensions': 0, // Do not enforce import file extensions.
  },
};
