import { useEffect } from 'react';
import userServices from '@/service/enterprise/user';
import { Select } from 'antd';
import { Outlet, history } from 'umi';
import { useStyles } from './style';
import { Icon } from '@chat2db/ui';
import Logo from '@/components/Logo';
import { MapPin } from 'lucide-react';
import { useGlobalStore } from '@/store/global';
import { isDesktop } from '@/utils/env';
import { JcefEventBus, JavaPushActionType } from '@/jcef/eventBus';
import { openWebPage } from '@/utils/url';

function unLoginLayout() {
  const { styles } = useStyles();
  const { appConfig, queryAppConfig } = useGlobalStore((state) => ({
    appConfig: state.appConfig,
    queryAppConfig: state.queryAppConfig,
  }));

  const { isReady } = appConfig;

  // switch country
  const handleChangeCountry = (value: string) => {
    const country = appConfig.countries?.find((item) => item.code === value);
    if (country) {
      userServices.setCountry({ country: country.code }).then(() => {
        if (isDesktop) {
          queryAppConfig();
        } else {
          openWebPage(country.appUrl, '_self');
        }
      });
    }
  };

  useEffect(() => {
    JcefEventBus.on(JavaPushActionType.OSS_LOGIN, () => {
      history.push('/');
    });

    return () => {
      JcefEventBus.off(JavaPushActionType.OSS_LOGIN);
    };
  }, []);

  return (
    <div className={styles.page}>
      <Logo className={styles.logo} type="imageWithText" size={36} />
      {isReady && <Outlet />}
      <div className={styles.settingBox}>
        <div className={styles.selectCountry}>
          <Icon icon={MapPin} size="sm" />
          <Select
            showSearch
            placeholder="Select your country"
            options={appConfig?.countries || []}
            fieldNames={{ label: 'name', value: 'code' }}
            optionFilterProp="name"
            value={appConfig.curCountry?.code}
            onChange={handleChangeCountry}
            popupMatchSelectWidth={false}
            popupClassName={styles.selectPopup}
            bordered={false}
          />
        </div>
      </div>
    </div>
  );
}

export default unLoginLayout;
