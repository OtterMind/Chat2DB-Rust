import { DivProps } from '@/typings/common';
import { createStyles } from 'antd-style';
import { ReactNode, memo } from 'react';

export interface PageTitleProps extends DivProps {
  title: string;
  postfix?: ReactNode;
}

export const useStyles = createStyles(({ css, token }) => {
  return {
    title: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-weight: 800;
      font-size: ${token.fontSizeHeading3}px;
      /* padding: 16px; */
      font-family: ${token.fontFamily};
    `,
    titleText: css`
      flex: 1;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    postfix: css`
      flex-shrink: 0;
      margin-left: 10px;
    `,
  };
});

const PageTitle = memo<PageTitleProps>(({ className, style, title, postfix, ...rest }) => {
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.title, className)} style={style} {...rest}>
      <div className={styles.titleText}>{title}</div>
      {postfix && <div className={styles.postfix}>{postfix}</div>}
    </div>
  );
});

export default PageTitle;
