import { memo } from 'react';
import { useStyles } from './style';
import { IconButton } from '@chat2db/ui';
import i18n from '@/i18n';
import { useUserStore } from '@/store/user';
import { useOrgStore } from '@/store/organization';
import { SubscriptionType } from '@/constants/subscriptionType';
import { OrganizationType } from '@/typings/enterprise/organization';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  const { setPricingModalStatus, setSubscriptType } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    setSubscriptType: s.setSubscriptType,
  }));

  const { curOrg } = useOrgStore((s) => ({
    curOrg: s.curOrg,
  }));

  const iconCode = curOrg?.vip ? 'icon-update' : 'icon-update';

  return (
    <>
      {!curOrg?.vip && (
        <IconButton
          className={cx(styles.upgradeButton, className)}
          title={i18n('common.text.upgrade')}
          tooltipPlacement="right"
          code={iconCode}
          size={{
            boxSize: 34,
            iconSize: 22,
          }}
          onClick={() => {
            setPricingModalStatus(true);
            setSubscriptType(
              curOrg?.type === OrganizationType.PERSONAL
                ? SubscriptionType.PersonalUpdate
                : SubscriptionType.TeamUpdate,
            );
            // setSubscriptType(SubscriptionType.TeamAddUser);
          }}
        />
      )}
    </>
  );
});
