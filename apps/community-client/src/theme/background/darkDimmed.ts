import { theme } from 'antd';
import { PrimaryColorType } from '@/constants';
import { commonToken } from '../common';

type IAntdPrimaryColor = {
  [key in PrimaryColorType]: any;
};

// theme color
const antdPrimaryColor: IAntdPrimaryColor = {
  [PrimaryColorType.Polar_Green]: {
    colorPrimary: '#3c8618',
  },
  [PrimaryColorType.Golden_Purple]: {
    colorPrimary: '#8276c9',
  },
  [PrimaryColorType.Polar_Blue]: {
    colorPrimary: '#1677ff',
  },
  [PrimaryColorType.Silver]: {
    colorPrimary: '#c3b7a4',
  },
  [PrimaryColorType.Red]: {
    colorPrimary: '#fd6874',
  },
  [PrimaryColorType.Orange]: {
    colorPrimary: '#ffa940',
  },
  [PrimaryColorType.Blue2]: {
    colorPrimary: '#009cc7',
  },
  [PrimaryColorType.Gold]: {
    colorPrimary: '#b59a6d',
  },
};

const antdLightTheme = {
  algorithm: [theme.darkAlgorithm, theme.compactAlgorithm],
  customName: 'dark-dimmed',
  antdPrimaryColor,
  token: {
    ...commonToken,
    colorTextBase: '#f1f1f4',
    colorBgBase: '#1c2128',
    colorHoverBg: 'hsla(0, 0%, 100%, 0.03)',
    colorBgContainer: '#1c2128',
    colorBgSubtle: '#22272e',
    colorBgElevated: '#1c2128',
    colorBorder: '#373e4766',
    colorBorderSecondary: '#373e4766',
    controlItemBgActive: '#f1f1f414',
    boxShadow:
      '0 6px 16px 0 rgba(241, 241, 244, 0.04), 0 3px 6px -4px rgba(241, 241, 244, 0.04), 0 9px 28px 8px rgba(241, 241, 244, 0.04)',
  },
};

export default antdLightTheme;
