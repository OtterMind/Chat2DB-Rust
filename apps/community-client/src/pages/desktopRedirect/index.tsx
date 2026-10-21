import { useEffect } from 'react';
import { Button } from 'antd';
import styles from './index.less';
import { openWebPage } from '@/utils/url';

const protocolScheme = 'chat2db-pro';

export default function DesktopRedirect() {
  useEffect(() => {
    openWebPage(`${protocolScheme}://connections`);
  }, []);

  return (
    <div className={styles.styles}>
      <Button
        onClick={() => {
          openWebPage(`${protocolScheme}://connections`);
        }}
      >
        Open Chat2DB
      </Button>
    </div>
  );
}
