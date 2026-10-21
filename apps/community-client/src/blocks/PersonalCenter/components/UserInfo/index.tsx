import { memo, useMemo } from 'react';
import { useStyles } from './style';
import Avatar from '@/components/Avatar';
import { IconfontSvg } from '@chat2db/ui';
import { Tooltip } from 'antd';
import i18n from '@/i18n';
import VipStatus from '@/components/VipStatus';
import { useUserStore } from '@/store/user';
import dayjs from 'dayjs';
import { useOrgStore } from '@/store/organization';

interface IProps {}

export default memo<IProps>(() => {
  const { styles } = useStyles();

  const { curUser } = useUserStore((s) => ({
    curUser: s.curUser,
  }));

  const { curOrgSubscription } = useOrgStore((state) => ({
    curOrgSubscription: state.curOrgSubscription,
    curOrg: state.curOrg,
    setCurOrg: state.setCurOrg,
    querySubscriptionList: state.querySubscriptionList,
  }));

  /** a founding member? */
  const isFoundingMember = useMemo(
    () => curOrgSubscription && dayjs(curOrgSubscription?.startTime).isBefore(dayjs('2024-05-08')),
    [curOrgSubscription?.startTime],
  );

  return (
    <div className={styles.userInfo}>
      <Avatar canEditor={false} />
      <div className={styles.userName}>
        <div className={styles.displayName}>{curUser?.displayName}</div>
        {isFoundingMember && (
          <Tooltip title={i18n('userguide.vipStatus.foundMember')} placement="right" mouseEnterDelay={0.2}>
            <div>
              <IconfontSvg code="icon-plus-1" className={styles.foreverIcon} />
            </div>
          </Tooltip>
        )}
      </div>
      <VipStatus size="sm" />
    </div>
  );
});
