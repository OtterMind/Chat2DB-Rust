import { memo } from 'react';
import { useStyles } from './style';
import { IconButton } from '@chat2db/ui';
import { CircleX } from 'lucide-react';

interface IProps {
  className?: string;
  errorMessage: string | null;
  handleClose?: () => void;
}

export default memo<IProps>(({ className, errorMessage, handleClose }) => {
  const { styles, cx } = useStyles();

  if (errorMessage === null) {
    return null;
  }

  return (
    <div className={cx(styles.monacoEditorError, className)}>
      <div className={styles.errorMessage}>{errorMessage || 'Unknown Error'}</div>
      <IconButton
        className={styles.closeButton}
        icon={CircleX}
        size="xs"
        onClick={() => {
          handleClose && handleClose();
        }}
      />
    </div>
  );
});
