import { DEFAULT_EDITOR_THEME } from '../constants';
import * as themes from '../theme';

const builtInThemes = {
  vs: 'vs',
  'vs-dark': 'vs-dark',
  'hc-light': 'hc-light',
  'hc-black': 'hc-black',
};

const customThemes = Object.fromEntries(
  Object.entries(themes).map(([key, value]) => {
    // Remove 'Theme' suffix
    const themeName = key.replace(/Theme$/, '');

    // Insert hyphen before capital letters, except the first one
    const hyphenatedName = themeName.replace(/(?!^)([A-Z][a-z]+)/g, '-$1');

    // Handle special case for "DB" which should not be split
    const finalName = hyphenatedName.replace(/(\d+)-([A-Z]+)/g, '$1$2');

    return [finalName, value];
  }),
);

const editorThemes = {
  ...customThemes,
  ...builtInThemes,
};

/**
 * Get the theme.
 * @param name
 * @returns
 */
const getTheme = (name: string): string => {
  return name in editorThemes ? name : DEFAULT_EDITOR_THEME;
};

export { getTheme, editorThemes };
