import { memo, useEffect, useState } from 'react';
import { useStyles } from './style';
import { IconButton } from '@chat2db/ui';
import { CircleX } from 'lucide-react';

interface IProps {
  className?: string;
  errorMessage: string | null;
  handleClose?: () => void;
}

export default memo<IProps>((props) => {
  const { className, errorMessage: _errorMessage, handleClose } = props;
  const { styles, cx } = useStyles();
  const [errorMessage, setErrorMessage] = useState<string | null>(_errorMessage);

  useEffect(() => {
    setErrorMessage(_errorMessage);
  }, [_errorMessage]);

  if (errorMessage === null) {
    return null;
  }

  return (
    <div className={cx(styles.monacoEditorError, className)}>
      <div className={styles.errorMessage}>{errorMessage}</div>
      <IconButton
        className={styles.closeButton}
        icon={CircleX}
        size="xs"
        onClick={() => {
          setErrorMessage(null);
          handleClose && handleClose();
        }}
      />
    </div>
  );
});
