import { useState } from 'react';
import i18n from '@/i18n';
import { Button, Input } from 'antd';
import { useStyles } from './style';
import outSideService from '@/service/outside';
import feedback from '@/utils/feedback';

// proxy settings
export default function ProxyBody() {
  const { styles } = useStyles();
  const [apiPrefix, setApiPrefix] = useState();

  function updateApi(e: any) {
    setApiPrefix(e.target.value);
  }

  function affirmUpdateApi() {
    if (!apiPrefix) {
      return;
    }
    outSideService
      .dynamicUrl(`${apiPrefix}/api/system/get-version-a`)
      .then(() => {
        location.reload();
      })
      .catch(() => {
        feedback.error(i18n('setting.message.urlTestError'));
      });
  }

  return (
    <>
      <div className={styles.title}>{i18n('setting.label.serviceAddress')}</div>
      <div className={styles.content}>
        <Input value={apiPrefix} onChange={updateApi} />
      </div>
      <div className={styles.bottomButton}>
        <Button type="primary" onClick={affirmUpdateApi}>
          {i18n('setting.button.apply')}
        </Button>
      </div>
    </>
  );
}
