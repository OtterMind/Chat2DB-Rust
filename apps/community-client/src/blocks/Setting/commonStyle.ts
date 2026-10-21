import { createStyles } from 'antd-style';

export const useCommonStyle = createStyles(({ css }) => {
  return {
    containerBlock: css`
      /* display: flex;
      gap: 30px;
      > :nth-of-type(1){
        flex: 1;
      }
      > :nth-of-type(2){
        flex: 2;
      } */
    `,
  };
});
