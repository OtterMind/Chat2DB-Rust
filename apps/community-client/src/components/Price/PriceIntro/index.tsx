import { useMemo } from 'react';
import UserInfo from '@/blocks/PersonalCenter/components/UserInfo';
import { useStyles } from './style';
import { Flex } from 'antd';
import i18n from '@/i18n';
import { IconfontSvg } from '@chat2db/ui';
import Logo from '@/components/Logo';
import { OrganizationType } from '@/typings/enterprise/organization';

import { isOfflineEnv } from '@/utils/env';
import { TabType } from '../PricePage';

interface IProps {
  className?: string;
  tabIndex?: OrganizationType | TabType;
}

const PriceIntro = ({ className, tabIndex }: IProps) => {
  const { styles, cx } = useStyles();

  const feature = useMemo(() => {
    if (isOfflineEnv || tabIndex === TabType.LOCAL) {
      return [
        {
          icon: 'icon-sparkles',
          title: i18n('userguide.offline.feature1.title'),
        },
        {
          icon: 'icon-database-nav',
          title: i18n('userguide.offline.feature2.title'),
        },
        {
          icon: 'icon-ai-table',
          title: i18n('userguide.offline.feature3.title'),
        },
        {
          icon: 'icon-terminal',
          title: i18n('userguide.offline.feature4.title'),
        },
        {
          icon: 'icon-table-view',
          title: i18n('userguide.offline.feature5.title'),
        },
        {
          icon: 'icon-run-sql',
          title: i18n('userguide.offline.feature6.title'),
        },
        {
          icon: 'icon-chart-square-bar',
          title: i18n('userguide.offline.feature7.title'),
        },
        {
          icon: 'icon-refresh',
          title: i18n('userguide.offline.feature8.title'),
        },
      ];
    }

    if (tabIndex === OrganizationType.PERSONAL || tabIndex === TabType.PERSONAL) {
      return [
        {
          icon: 'icon-sparkles',
          title: i18n('userguide.personal.feature1.title'),
        },
        {
          icon: 'icon-database-nav',
          title: i18n('userguide.personal.feature2.title'),
        },
        {
          icon: 'icon-ai-table',
          title: i18n('userguide.personal.feature3.title'),
        },
        {
          icon: 'icon-terminal',
          title: i18n('userguide.personal.feature4.title'),
        },
        {
          icon: 'icon-table-view',
          title: i18n('userguide.personal.feature5.title'),
        },
        {
          icon: 'icon-run-sql',
          title: i18n('userguide.personal.feature6.title'),
        },
        {
          icon: 'icon-chart-square-bar',
          title: i18n('userguide.personal.feature7.title'),
        },
        {
          icon: 'icon-refresh',
          title: i18n('userguide.personal.feature8.title'),
        },
      ];
    }

    if (tabIndex === OrganizationType.TEAM || tabIndex === TabType.TEAM) {
      return [
        {
          icon: 'icon-terminal1',
          title: i18n('userguide.team.feature1.title'),
        },
        {
          icon: 'icon-a-xunwen1',
          title: i18n('userguide.team.feature2.title'),
        },
        {
          icon: 'icon-user-group',
          title: i18n('userguide.team.feature3.title'),
        },
        {
          icon: 'icon-file-exchange',
          title: i18n('userguide.team.feature4.title'),
        },
        {
          icon: 'icon-yulan1',
          title: i18n('userguide.team.feature5.title'),
        },
        {
          icon: 'icon-database2',
          title: i18n('userguide.team.feature6.title'),
        },
        {
          icon: 'icon-archive',
          title: i18n('userguide.team.feature7.title'),
        },
      ];
    }
  }, [tabIndex]);

  const dividerText = useMemo(() => {
    if (isOfflineEnv || tabIndex === TabType.LOCAL) {
      return i18n('price.text.offlinePro');
    } else {
      return i18n('price.text.upgradePro');
    }
  }, [isOfflineEnv, tabIndex]);

  return (
    <div className={cx(styles.wrapper, className)}>
      {isOfflineEnv ? (
        <div style={{ marginTop: '24px' }}>
          <Logo size={42} />
        </div>
      ) : (
        <UserInfo />
      )}
      <div className={styles.divideline}>{dividerText}</div>
      <Flex gap={20} vertical align="start">
        {(feature || []).map(({ icon, title }, id) => (
          <Flex key={id} gap={12} align="center">
            <Flex className={styles.featureIcon} justify="center" align="center">
              <IconfontSvg code={icon} size={24} className={styles.svgIcon} />
            </Flex>
            <div className={styles.featureTitle}>{title}</div>
          </Flex>
        ))}
      </Flex>
    </div>
  );
};

export default PriceIntro;
