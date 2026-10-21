import React, { FC } from 'react';
import { createStyles } from 'antd-style';
import Iconfont from '@/components/Iconfont';

export interface DBBoxProps extends React.HTMLAttributes<HTMLDivElement> {
  name: string;
  // \uec21
  icon: string;
}

const useStyles = createStyles(({ css, cx, token }) => {
  const boxRight = cx(css`
    opacity: 0;
    i {
      font-size: 16px;
    }
  `);

  return {
    dbBox: css`
      height: 50px;
      width: 210px;

      padding: 0px 16px;
      border-radius: 8px;
      border: 1px solid ${token.colorBorder};

      display: flex;
      justify-content: space-between;
      align-items: center;
      cursor: pointer;
      &:hover {
        border: 1px solid ${token.colorPrimary};
        background-color: ${token.colorPrimaryBg};
        color: ${token.colorPrimary};
        transition: all 400ms ${token.motionEaseOut};
        .${boxRight} {
          opacity: 1;
          transition: opacity 400ms ${token.motionEaseOut};
        }
      }
      &:active {
        transition: scale 400ms ${token.motionEaseOut};
        scale: 0.95;
      }
    `,
    boxLeft: css`
      display: flex;
      align-items: center;
      justify-content: start;
    `,
    logoBox: css`
      display: flex;
      justify-content: center;
      align-items: center;
      height: 28px;
      width: 28px;
      border-radius: 8px;
      margin-right: 16px;

      i {
        font-size: 16px;
      }
    `,
    boxRight,
  };
});

const DBBox: FC<DBBoxProps> = ({ name, icon, onClick }) => {
  const { styles } = useStyles();
  return (
    <div className={styles.dbBox} key={name} onClick={onClick}>
      <div className={styles.boxLeft}>
        <div className={styles.logoBox}>
          <Iconfont code={icon} />
        </div>
        {name}
      </div>
      <div className={styles.boxRight}>
        <Iconfont code="&#xe631;" />
      </div>
    </div>
  );
};

export default DBBox;
