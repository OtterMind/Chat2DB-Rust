import { memo } from 'react';
import { IconButton } from '@chat2db/ui';
import { refreshPage } from '@/utils';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  return <IconButton className={className} code="icon-refresh" onClick={refreshPage} />;
});
