import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, cx, token }) => {
  const cascaderDelete = cx(css`
    position: absolute;
    right: 0;
    opacity: 0;
    transition: opacity 400ms ${token.motionEaseInBack};
  `);
  return {
    container: css`
      display: flex;
      flex-direction: column;
      gap: 8px;
    `,
    cascaderDiv: css`
      position: relative;
      &:hover {
        .${cascaderDelete} {
          opacity: 1;
        }
      }
    `,
    cascaderDelete,
  };
});
