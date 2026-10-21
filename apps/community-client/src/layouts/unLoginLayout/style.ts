import { createStyles } from 'antd-style';
import backgroundImg from '@/assets/img/bg.webp';

export const useStyles = createStyles(({ css }) => {
  return {
    page: css`
      height: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
      background-image: url(${backgroundImg});
      background-size: cover;
      background-position: center center;
      background-repeat: no-repeat;
    `,
    logo: css`
      position: absolute;
      top: 42px;
      left: 32px;
      color: #24272d;
    `,
    setting: css`
      position: absolute;
      bottom: 32px;
      left: 32px;
      height: auto;
      width: auto;
    `,
    settingBtn: css`
      padding: 0px 12px;
      font-size: 14px;
      opacity: 0.8;

      &:hover {
        opacity: 1;
      }

      &:active {
        scale: 0.9;
      }
    `,
    settingBox: css`
      position: absolute;
      bottom: 32px;
      left: 32px;
      right: 0px;
    `,
    selectCountry: css`
      display: flex;
      align-items: center;
      width: fit-content;
      color: #24272d;
      .ant-select, .ant-select-selection-item, input {
        color: #24272d !important;
      }
    `,
    selectPopup: css`
      width: 200px;
    `,
  };
});
