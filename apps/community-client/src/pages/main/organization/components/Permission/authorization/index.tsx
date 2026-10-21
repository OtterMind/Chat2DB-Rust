import { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Drawer, Space, Tag } from 'antd';
import {
  AuthSubjectType,
  AuthType,
  IAccessControlAuthRecordVO,
  IAuthAdminRequest,
  IAuthDataAccessRequest,
} from '@/typings/enterprise/permission';
import { PlusOutlined } from '@ant-design/icons';
import permissionService from '@/service/enterprise/permission';
import { useOrgStore } from '@/store/organization';
import AuthAdminForm, { IAdminFrom } from '../components/adminForm';
import DataAccessForm, { IDataAccessForm } from '../components/dataAccessForm';
import dayjs from 'dayjs';
import i18n from '@/i18n';
import { BooleanType } from '@/typings/common';
import styles from './index.less';
import PageTitle from '@/components/PageTitle';
import AntdTable from '@/components/AntdTable';
import { staticMessage } from '@chat2db/ui';

/** Authorization. */
function Authorization() {
  const [dataSource, setDataSource] = useState<IAccessControlAuthRecordVO[]>([]);
  const [pagination, setPagination] = useState({
    searchKey: '',
    current: 1,
    pageSize: 10,
    total: 0,
  });
  const [authType, setAuthType] = useState<AuthType>(AuthType.Admin);
  const [isDrawerVisible, setIsDrawerVisible] = useState(false);
  const authAdminFormRef = useRef<IAdminFrom>();
  const authDataAccessFormRef = useRef<IDataAccessForm>();

  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));

  const columns = useMemo(
    () => [
      {
        title: i18n('team.authorization.table.name'),
        dataIndex: 'name',
        key: 'name',
      },
      {
        title: i18n('team.authorization.table.desc'),
        dataIndex: 'description',
        key: 'description',
      },
      {
        title: i18n('team.authorization.table.type'),
        dataIndex: 'authType',
        key: 'authType',
        render: (type: AuthType) => (
          <>
            {[AuthType.OWNER, AuthType.Admin].includes(type) ? (
              <Tag color="blue">{i18n('team.authorization.authAdmin')}</Tag>
            ) : (
              <Tag color="green">{i18n('team.authorization.authData')}</Tag>
            )}
          </>
        ),
      },
      {
        title: i18n('team.authorization.table.validity'),
        dataIndex: 'valid',
        key: 'valid',
        render: (_: any, record: IAccessControlAuthRecordVO) => {
          if (record.noExpire === BooleanType.Yes) {
            return <span>{i18n('team.authorization.table.validity.forever')}</span>;
          }
          return <span>{record?.validUntil ? dayjs(record?.validUntil).format('YYYY-MM-DD') : null}</span>;
        },
      },
      {
        title: i18n('team.authorization.table.action'),
        key: 'action',

        width: 80,
        render: (_: any, record: IAccessControlAuthRecordVO) => (
          <>
            <Button
              type="link"
              onClick={async () => {
                if ([AuthType.Admin, AuthType.OWNER].includes(record.authType)) {
                  handleQueryAdminDetail(record);
                }
                if (AuthType.OPERATOR === record.authType) {
                  handleQueryDataAccessDetail(record);
                }
              }}
            >
              {i18n('team.authorization.action.view')}
            </Button>
          </>
        ),
      },
    ],
    [],
  );

  const isPreivew = useMemo(() => {
    if ([AuthType.Admin, AuthType.OWNER].includes(authType)) {
      return !!authAdminFormRef.current?.id;
    }

    if (AuthType.OPERATOR === authType) {
      return !!authDataAccessFormRef.current?.id;
    }
  }, [authType, authAdminFormRef.current, authDataAccessFormRef.current]);

  useEffect(() => {
    queryAuthList();
  }, [pagination.searchKey, pagination.current, pagination.pageSize]);

  const queryAuthList = async () => {
    const { searchKey, current: pageNo, pageSize } = pagination;
    const res = await permissionService.queryAuthList({
      searchKey,
      pageNo,
      pageSize,
      organizationId: curOrg?.id as number,
    });
    if (res) {
      setDataSource(res?.data ?? []);
      setPagination({
        ...pagination,
        total: res.total,
      });
    }
  };

  const handleTableChange = (p) => {
    setPagination({
      ...pagination,
      ...p,
    });
  };

  const handleCancelDrawer = () => {
    setIsDrawerVisible(false);
  };

  const handleSumbitDrawer = async () => {
    if (!curOrg?.id) return;

    if ([AuthType.Admin, AuthType.OWNER].includes(authType)) {
      handleAuthAdmin();
    }

    if (AuthType.OPERATOR === authType) {
      handleAuthDataAccess();
    }
  };

  /**
   * Grant administrator permissions.
   */
  const handleAuthAdmin = async () => {
    if (!authAdminFormRef.current || !curOrg?.id) {
      return;
    }
    // TODO: Validate.
    const { name, description, userId, noExpire, validUntil, dataSourceId } = authAdminFormRef.current;
    const authAdminFormParams: IAuthAdminRequest = {
      organizationId: curOrg?.id,
      name,
      description,
      noExpire,
      validUntil,
      userId: userId as number,
      dataSourceId: dataSourceId as number,
      authType: AuthType.Admin,
      authSubjectType: AuthSubjectType.USER,
    };
    try {
      await permissionService.authAdmin(authAdminFormParams);
      staticMessage.success(i18n('team.authorization.success.tip'));
      setIsDrawerVisible(false);
      queryAuthList();
    } catch (error) {
      console.log(error);
    }
  };

  /**
   * Query administrator authorization details.
   */
  const handleQueryAdminDetail = async (record: IAccessControlAuthRecordVO) => {
    const res = await permissionService.queryAuthDetail({ id: record.id });
    if (res) {
      const { id, name, description, authUser, dataSourceAdminList, noExpire, validUntil } = res;
      authAdminFormRef.current = {
        id,
        name,
        description,
        userId: { value: authUser.id, label: authUser.displayName },
        dataSourceId: {
          value: dataSourceAdminList?.[0]?.dataSourceId,
          label: dataSourceAdminList?.[0]?.dataSourceName,
        },
        noExpire,
        validUntil,
      };
      setIsDrawerVisible(true);
      setAuthType(AuthType.Admin);
    }
  };

  /**
   * Grant data permissions.
   */
  const handleAuthDataAccess = async () => {
    if (!authDataAccessFormRef.current || !curOrg?.id) return;

    // TODO: Validate.
    const {
      name,
      description,
      userId,
      noExpire,
      validUntil,
      isAllSchema,
      dataSourceId,
      databaseInfo,
      rowFilter,
      rowCount,
      policyVOList,
    } = authDataAccessFormRef.current || {};
    const { databaseName, schemaName, tableName, columnNames } = databaseInfo || {};
    const authDataAccessFormParams: IAuthDataAccessRequest = {
      organizationId: curOrg?.id,
      name,
      description,
      userId,
      noExpire,
      validUntil,
      isAllSchema,
      rowCount,
      rowFilter,
      policyVOList,
      dataSourceId,
      databaseName,
      schemaName,
      tableName,
      // Pass an empty array for "all"; the backend interprets it as full access.
      columnNames: (columnNames || []).includes('all') ? [] : columnNames,
    };
    await permissionService.authDataPermission(authDataAccessFormParams);
    setIsDrawerVisible(false);
    queryAuthList();
  };

  /**
   * Query data-permission details.
   */
  const handleQueryDataAccessDetail = async (record: IAccessControlAuthRecordVO) => {
    const res = await permissionService.queryAuthDetail({ id: record.id });
    if (res) {
      const {
        id,
        name,
        description,
        authUser,
        dataAccessControlList,
        noExpire,
        validUntil,
        policyVOList,
        columnNames,
      } = res;
      const { dataSourceId, databaseName, tableName, schemaName, rowCount, rowFilter } =
        dataAccessControlList?.[0] || {};
      authDataAccessFormRef.current = {
        id,
        name,
        description,
        userId: authUser.id,
        // Check whether the authorization covers the entire database.
        isAllSchema: databaseName === 'ALL_DATABASE_GRANTED',
        dataSourceId,
        databaseInfo: {
          databaseName,
          tableName,
          schemaName,
          columnNames: columnNames.includes('ALL_COLUMN_GRANTED') ? [''] : columnNames,
        },
        noExpire,
        validUntil,
        policyVOList,
        rowCount,
        rowFilter,
      };

      setAuthType(AuthType.OPERATOR);
      setIsDrawerVisible(true);
    }
  };

  return (
    <div className={styles.wrapper}>
      <PageTitle title={i18n('team.nav.permission.authorization')} />
      <div className={styles.tableTop}>
        {/* <Input.Search
          style={{ width: '320px' }}
          // placeholder={i18n('team.input.search.placeholder')}
          onSearch={handleSearch}
          enterButton={<SearchOutlined />}
        /> */}
        <div />
        <div>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => {
              setIsDrawerVisible(true);
              setAuthType(AuthType.Admin);
              authAdminFormRef.current = undefined;
            }}
            style={{ marginRight: '8px' }}
          >
            {/* Grant administrator permissions. */}
            {i18n('team.authorization.authAdmin')}
          </Button>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => {
              setIsDrawerVisible(true);
              setAuthType(AuthType.OPERATOR);
              authDataAccessFormRef.current = undefined;
            }}
          >
            {/* Grant data permissions. */}
            {i18n('team.authorization.authData')}
          </Button>
        </div>
      </div>
      <AntdTable
        className={styles.antdTable}
        rowKey={'id'}
        dataSource={dataSource}
        columns={columns}
        pagination={pagination}
        onChange={handleTableChange}
      />

      <Drawer
        width={860}
        className={styles.authDrawer}
        title={
          [AuthType.OWNER, AuthType.Admin].includes(authType)
            ? i18n('team.authorization.authAdmin')
            : i18n('team.authorization.authData')
        }
        open={isDrawerVisible}
        onClose={handleCancelDrawer}
        destroyOnClose
        extra={
          isPreivew ? null : (
            <Space>
              <Button onClick={handleCancelDrawer}>{i18n('common.button.cancel')}</Button>

              <Button type="primary" onClick={handleSumbitDrawer}>
                {i18n('common.button.submit')}
              </Button>
            </Space>
          )
        }
      >
        {[AuthType.Admin, AuthType.OWNER].includes(authType) && (
          <AuthAdminForm
            isPreview={isPreivew}
            initData={authAdminFormRef.current}
            onAuthFormChange={(v) => {
              authAdminFormRef.current = v;
            }}
          />
        )}
        {authType === AuthType.OPERATOR && (
          <DataAccessForm
            type="auth"
            isPreview={isPreivew}
            initData={authDataAccessFormRef.current}
            onFormChange={(v) => {
              authDataAccessFormRef.current = v;
            }}
          />
        )}
      </Drawer>
    </div>
  );
}

export default Authorization;
