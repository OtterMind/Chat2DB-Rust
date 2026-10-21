import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import { openWebPage } from '@/utils/url';
import { IconfontSvg, MenuLabel } from '@chat2db/ui';
import { Dropdown, Tooltip } from 'antd';
import React, { memo, useMemo, useState } from 'react';
import { history } from 'umi';
import { useStyles } from './style';

import Avatar from '@/components/Avatar';
import Feedback from '@/components/Feedback';
import NotificationButton from '@/components/NotificationNav';
import VipStatus from '@/components/VipStatus';
import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import i18n from '@/i18n';
import userServices from '@/service/enterprise/oauth';
import { useOrgStore } from '@/store/organization';
import { IOrganizationVO, OrganizationType } from '@/typings/enterprise/organization';
import dayjs from 'dayjs';

interface PersonalCenterProps {
  /** Avatar trigger size (px), default 36 */
  triggerSize?: number;
  children?: React.ReactNode;
}

export default memo(({ triggerSize = 36, children }: PersonalCenterProps) => {
  const { cx, styles } = useStyles();
  const [openDropdown, setOpenDropdown] = useState<undefined | false>(undefined);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const [notificationOpen, setNotificationOpen] = useState(false);
  const { setSettingPageActiveTab } = useGlobalStore((state) => {
    return {
      setSettingPageActiveTab: state.setSettingPageActiveTab,
    };
  });
  const {
    curIsPersonalOrg,
    curOrgSubscription,
    setCurOrg,
    curOrg,
    orgList,
    setOpenCreateOrJoinOrgDialog,
    queryOrgList,
  } = useOrgStore((state) => ({
    curIsPersonalOrg: state.curIsPersonalOrg,
    curOrgSubscription: state.curOrgSubscription,
    curOrg: state.curOrg,
    setCurOrg: state.setCurOrg,
    orgList: state.orgList,
    setOpenCreateOrJoinOrgDialog: state.setOpenCreateOrJoinOrgDialog,
    queryOrgList: state.queryOrgList,
  }));

  const { queryCurUser, curUser } = useUserStore((s) => ({
    queryCurUser: s.queryCurUser,
    curUser: s.curUser,
  }));

  /** a founding member? */
  const isFoundingMember = useMemo(
    // The current organization exists, and the organization's subscription time is before May 8, 2024
    () => curOrg && curOrgSubscription && dayjs(curOrgSubscription?.startTime).isBefore(dayjs('2024-05-08')),
    [curOrgSubscription?.id],
  );

  const handleLogout = () => {
    userServices.userLogout().then(() => {
      history.push('/login');
    });
  };

  const renderOrgItem = (org: IOrganizationVO, checked?: boolean) => {
    const isFree = !org?.vip;
    return (
      <div
        className={cx(styles.orgItem)}
        onClick={() => {
          setCurOrg(org);
        }}
      >
        <div className={styles.orgItemLeft}>
          <Avatar org={org} size={32} />
          <div className={styles.orgItemName}>{org.name}</div>
          <div className={styles.orgItemTag}>{isFree ? 'Free' : 'Pro'}</div>
        </div>
        {!!checked && <IconfontSvg code="icon-danse" size={16} />}
      </div>
    );
  };

  const renderOrgDropdown = () => {
    const teamList = orgList?.filter((org) => org.type === OrganizationType.TEAM);
    return (teamList || []).map((org) => ({
      key: org.id.toString(),
      label: renderOrgItem(org, org.id === curOrg?.id),
    }));
  };

  const items = useMemo(() => {
    const menuItems: any[] = [
      {
        key: 'personal',
        icon: <IconfontSvg code="icon-user-circle" />,
        label: <MenuLabel label={i18n('setting.label.personal')} />,
        onClick: () => {
          setSettingPageActiveTab('personal');
        },
      },
      {
        key: 'setting',
        icon: <IconfontSvg code="icon-adjustments" />,
        label: <MenuLabel label={i18n('setting.title.setting')} />,
        onClick: () => {
          setSettingPageActiveTab('basic');
        },
      },
      {
        type: 'divider' as any,
      },
      // {
      //   key: 'github',
      //   icon: <IconfontSvg code="icon-GitHub" />,
      //   label: <MenuLabel label={'Github'} />,
      //   onClick: () => {
      //     openWebPage('https://github.com/chat2db/Chat2DB');
      //   },
      // },
      // {
      //   key: 'badgeHelp',
      //   icon: <IconfontSvg code="icon-chat-alt-2" />,
      //   label: <MenuLabel label={i18n('setting.label.requestFeature')} />,
      //   onClick: () => {
      //     openWebPage('https://github.com/chat2db/Chat2DB/issues');
      //   },
      // },
      {
        key: 'quickStart',
        icon: <IconfontSvg code="icon-book-open" />,
        label: <MenuLabel label={i18n('setting.label.quickStart')} />,
        onClick: () => {
          openWebPage('https://docs.chat2db.ai/docs/start-guide/getting-started');
        },
      },
      {
        type: 'divider' as any,
      },
      {
        key: 'createOrg',
        icon: <IconfontSvg code="icon-a-xunwen1" />,
        label: (
          <MenuLabel label={i18n('setting.nav.createOrgJoinOrg')} slot={<div className={styles.menuSlot}>New</div>} />
        ),
        onClick: () => {
          setOpenCreateOrJoinOrgDialog(true);
        },
      },
      {
        type: 'divider' as any,
      },
      {
        key: '3',
        icon: <IconfontSvg code="icon-logout" />,
        label: <MenuLabel label={i18n('login.label.signOut')} />,
        onClick: handleLogout,
      },
    ];

    const onlineMenuItems = [
      runtimeEditionConfig.accountCenter
        ? {
            key: 'notification',
            icon: <IconfontSvg code="icon-bell" />,
            label: <MenuLabel label={i18n('notification.title')} />,
            onClick: () => {
              setOpenDropdown(false);
              setTimeout(() => {
                setNotificationOpen(true);
                setOpenDropdown(undefined);
              });
            },
          }
        : null,
      runtimeEditionConfig.feedbackEntry
        ? {
            key: 'feedback',
            icon: <IconfontSvg code="icon-annotation" />,
            label: <MenuLabel label={i18n('feedback.title')} />,
            onClick: () => {
              setOpenDropdown(false);
              setTimeout(() => {
                setFeedbackOpen(true);
                setOpenDropdown(undefined);
              });
            },
          }
        : null,
    ].filter(Boolean) as any[];

    if (onlineMenuItems.length > 0) {
      const settingDividerIndex = menuItems.findIndex((item) => item.type === 'divider');
      menuItems.splice(settingDividerIndex + 1, 0, ...onlineMenuItems);
    }

    // Add the team switcher before the create/join action when a team organization exists.
    if (orgList?.find((org) => org.type === OrganizationType.TEAM)) {
      const index = menuItems.findIndex((item) => item.key === 'createOrg');
      menuItems.splice(index, 0, {
        key: 'switchTeam',
        icon: <IconfontSvg code="icon-switch-horizontal" />,
        label: <MenuLabel label={i18n('setting.nav.switchOrg')} />,
        children: renderOrgDropdown(),
      });
    }

    // In a team organization, add the personal switcher after the create/join action.
    if (!curIsPersonalOrg()) {
      // Find index based on key
      const index = menuItems.findIndex((item) => item.key === 'createOrg');
      menuItems.splice(index + 1, 0, {
        key: 'switchPersonal',
        icon: <IconfontSvg code="icon-user-circle" />,
        label: <MenuLabel label={i18n('setting.nav.switchPersonal')} />,
        onClick: () => {
          const personalOrg = orgList?.find((org) => org.type === OrganizationType.PERSONAL);
          setCurOrg(personalOrg);
        },
      });
    }
    return menuItems;
  }, [curOrg?.id, orgList]);

  const handleClick = () => {
    setOpenDropdown(false);
    setTimeout(() => {
      setOpenDropdown(undefined);
    });
  };

  const dropdownRender = (menu: React.ReactNode) => {
    return (
      <div className={styles.dropdownBox}>
        <div className={styles.userInfo}>
          <Avatar canEditor={true} />
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
          {!curIsPersonalOrg() && <div className={styles.displayOrg}>{curOrg?.name || '暂无组织'}</div>}
          <VipStatus size="sm" handleClick={handleClick} />
        </div>
        {menu}
      </div>
    );
  };

  const handleOpenChange = (isOpen: boolean) => {
    if (!isOpen) return;
    queryCurUser();
    queryOrgList();
  };

  return (
    <>
      <Dropdown
        open={openDropdown}
        menu={{ items, defaultSelectedKeys: [Number(curOrg?.id).toString()] }}
        trigger={['click']}
        dropdownRender={dropdownRender}
        placement={'topRight'}
        destroyPopupOnHide
        onOpenChange={handleOpenChange}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flex: 1, minWidth: 0, cursor: 'pointer' }}>
          <Avatar size={triggerSize} />
          {children}
        </div>
      </Dropdown>
      {runtimeEditionConfig.feedbackEntry && <Feedback open={feedbackOpen} onClose={() => setFeedbackOpen(false)} />}
      {runtimeEditionConfig.accountCenter && (
        <NotificationButton drawerMode open={notificationOpen} onClose={() => setNotificationOpen(false)} />
      )}
    </>
  );
});

export { default as OfflineAvatar } from './components/OfflineAvatar';
