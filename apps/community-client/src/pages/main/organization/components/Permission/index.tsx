import { useEffect, useMemo, useState } from 'react';
import { FileProtectOutlined, DatabaseOutlined } from '@ant-design/icons';
import { Flex, Tabs } from 'antd';
import Authorization from './authorization';
import Apply from './apply';
import DataMarket from './dataMarket';
import { getAllUrlParams } from '@/utils/url';
import { PermissionType, PermissionTypeName } from '@/typings/enterprise/permission';
// ----- store -----
import { useOrgStore } from '@/store/organization';

import styles from './index.less';
import { OrganizationType } from '@/typings/enterprise/organization';

function Permission() {
  const [activeKey, setActiveKey] = useState<PermissionType>(PermissionType.DATA_MARKET);

  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));

  useEffect(() => {
    const params = getAllUrlParams();
    // tab=apply&form=script
    // Set the default tab.
    if (params.tab) {
      setActiveKey(params.tab as PermissionType);
    }
  }, []);

  const tabList = useMemo(() => {
    const isEnterprise = curOrg?.type === OrganizationType.ENTERPRISE;
    let tabs: any[] = [
      {
        key: PermissionType.DATA_MARKET,
        label: PermissionTypeName.DATA_MARKET,
        icon: <FileProtectOutlined />,
        children: <DataMarket />,
      },
    ];
    if (isEnterprise) {
      tabs = tabs.concat([
        {
          key: PermissionType.AUTH,
          label: PermissionTypeName.AUTH,
          icon: <FileProtectOutlined />,
          children: <Authorization />,
        },
        {
          key: PermissionType.APPLY,
          label: PermissionTypeName.APPLY,
          icon: <DatabaseOutlined />,
          children: <Apply />,
        },
      ]);
    }
    return tabs;
  }, [curOrg?.type]);

  const curTab = tabList.find((tab) => tab.key === activeKey);

  return (
    <div className={styles.wrapper}>
      <div className={styles.header}>权限管理-{curTab.label || ''}</div>
      <Tabs
        activeKey={activeKey}
        onChange={(key: string) => {
          setActiveKey(key as PermissionType);
        }}
        items={tabList.map(({ key, icon, label, children }) => ({
          key: key,
          label: (
            <Flex gap={8}>
              {icon}
              {label}
            </Flex>
          ),
          children: children,
        }))}
      />
    </div>
  );
}

export default Permission;
