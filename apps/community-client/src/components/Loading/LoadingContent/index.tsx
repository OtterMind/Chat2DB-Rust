import React from 'react';
import classnames from 'classnames';
import StateIndicator from '@/components/StateIndicator';
import { useStyles } from './style';

// IProps extends native div attributes.
interface IProps<T> extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  data?: T | null | undefined | true;
  empty?: React.ReactNode;
  handleEmpty?: boolean;
  isLoading?: boolean;
  coverLoading?: boolean;
}

export default function LoadingContent<T>(props: IProps<T>) {
  const { styles } = useStyles();
  const { children, className, data = true, handleEmpty = false, empty, isLoading, coverLoading } = props;
  const isEmpty = !isLoading && handleEmpty && !(data as any)?.length;

  const renderContent = () => {
    if ((isLoading || !data) && !coverLoading) {
      return <StateIndicator state="loading" />;
    }

    if (isEmpty) {
      return empty || <StateIndicator state="empty" />;
    }

    return (
      <>
        {children}
        {(isLoading || !data) && coverLoading && (
          <div className={styles.coverLoading}>
            <StateIndicator state="loading" />
          </div>
        )}
      </>
    );
  };

  return <div className={classnames(styles.loadingContent, className)}>{renderContent()}</div>;
}
