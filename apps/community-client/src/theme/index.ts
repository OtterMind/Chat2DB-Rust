import antdDarkTheme from './background/dark';
import { ThemeAppearance } from '@chat2db/ui';
import antdDarkDimmedTheme from './background/darkDimmed';
import antdLightTheme from './background/light';
import { PrimaryColorType } from '@/constants';
import lodash from 'lodash';
import { theme } from 'antd';

export interface ITheme {
  backgroundColor: ThemeAppearance;
  primaryColor: PrimaryColorType;
}

const antdThemeConfigs = {
  [ThemeAppearance.Dark]: antdDarkTheme,
  [ThemeAppearance.Light]: antdLightTheme,
  [ThemeAppearance.DarkDimmed]: antdDarkDimmedTheme,
};

export function getAntdThemeConfig(_theme: ITheme) {
  const antdThemeConfig = lodash.cloneDeep(antdThemeConfigs[_theme.backgroundColor]);
  antdThemeConfig.token = {
    ...antdThemeConfig.token,
    ...(antdThemeConfig.antdPrimaryColor[_theme.primaryColor as PrimaryColorType] || {}),
  };

  const token = theme.getDesignToken(antdThemeConfig);
  injectThemeVar(token as any, _theme.backgroundColor, _theme.primaryColor);
  return antdThemeConfig;
}

// TODO: insert only once
export function injectThemeVar(
  token: { [key in string]: string },
  _theme: ThemeAppearance,
  primaryColor: PrimaryColorType,
) {
  let css = '';
  Object.keys(token).map((t) => {
    const attributeName = camelToDash(t);
    let value = token[t];
    // Bring the number that needs px to px
    const joinPxArr = ['fontSize', 'borderRadius', 'borderRadiusLG'];
    if (joinPxArr.includes(t)) {
      value = value + 'px';
    }
    css = css + `--${attributeName}: ${value};\n`;
  });

  const container = `html[theme='${_theme}'],html[primary-color='${primaryColor}']{
    ${css}
  }`;

  const style = document.createElement('style'); // Create style tag
  style.type = 'text/css';
  style.appendChild(document.createTextNode(container));

  document.head.appendChild(style); // Insert the style tag into the head tag
  window._AppThemePack = token;
}

function camelToDash(str: string) {
  return str.replace(/([A-Z])/g, '-$1').toLowerCase();
}
