import { createStyles } from 'antd-style';
import { IVipStatus, VIP_TYPE } from '.';

export const useStyles = createStyles(
  (
    { css, token },
    { size, vipType, isOverseas }: { size: IVipStatus['size']; vipType: VIP_TYPE; isOverseas: boolean },
  ) => {
    const { fontSize, lineHeight } = sizeCalc(size);

    let editionBg = token.colorPrimary;
    let editionColor = token.colorBgBase;
    let validDateColor = token.colorPrimaryActive;
    let validDateBg = token.colorPrimaryBgHover;

    if (vipType === VIP_TYPE.FOREVER) {
  // Permanent.
      editionBg = '#C78A2A';
      validDateColor = '#A27122';
      validDateBg = '#FCF0CC';
    } else if (vipType === VIP_TYPE.TRIAL) {
  // Trial.
      editionBg = token.colorTextSecondary;
      editionColor = token.colorTextQuinary;
      validDateColor = token.colorText;
      validDateBg = token.colorFillTertiary;
    }

    return {
      wrapper: css`
        font-size: ${fontSize}px;
        line-height: ${lineHeight};
      `,
      edition: css`
        background-color: ${editionBg};
        color: ${editionColor};
        padding: 0 6px;
        border-radius: ${isOverseas ? '4px' : '4px 0 0 4px'};
      `,
      validDate: css`
        color: ${validDateColor};
        background-color: ${validDateBg};
        padding: 0 6px;
        border-radius: 0 4px 4px 0;
        display: flex;
        align-items: center;
        gap: 4px;
        cursor: ${vipType === VIP_TYPE.TRIAL ? 'pointer' : 'auto'};
      `,
    };
  },
);

const sizeCalc = (size: IVipStatus['size']) => {
  const sizeStore = {
    sm: {
      fontSize: 12,
      lineHeight: 1.6,
    },
    md: {
      fontSize: 14,
      lineHeight: 1.4,
    },
    lg: {
      fontSize: 16,
      lineHeight: 1.2,
    },
  };

  return sizeStore[size ?? 'md'];
};
