import React, { memo } from 'react';
import { Loading } from '@chat2db/ui';

// IProps extends native div attributes.
interface IProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  isLoading?: boolean;
  empty?: React.ReactNode;
  isEmpty?: boolean;
  coverLoading?: boolean;
}

export default memo<IProps>((props) => {
  const { children, empty, isLoading, isEmpty } = props;

  if (isEmpty) {
    return empty;
  }

  if (isLoading) {
    return <Loading />;
  }

  return children;
});
