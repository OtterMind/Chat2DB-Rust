import { useEffect, useRef, useState } from 'react';
import { Button, Drawer, Space, Tabs } from 'antd';
import permissionService from '@/service/enterprise/permission';
import ScriptTable from './script-table';
import DataTable from './data-table';
import ScriptForm, { IScriptForm } from '../components/scriptForm';
import DataAccessForm, { IDataAccessForm } from '../components/dataAccessForm';
import { useOrgStore } from '@/store/organization';
import { IAccessControlApplyRecordVO, IApplyDataAccessRequest } from '@/typings/enterprise/permission';
import { BooleanType } from '@/typings';
import styles from './index.less';
import PageTitle from '@/components/PageTitle';
import i18n from '@/i18n';
import { staticMessage } from '@chat2db/ui';
import feedback from '@/utils/feedback';

export enum ApplyType {
  Script = 'script',
  Data = 'data',
  Import = 'import',
}

function Apply() {
  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));
  const [applyType, setApplyType] = useState<ApplyType>(ApplyType.Data);
  const [isDrawerVisible, setIsDrawerVisible] = useState(false);
  const dataFormValuesRef = useRef<IDataAccessForm>();
  const scriptFormValuesRef = useRef<IScriptForm>();

  const dataTableRef = useRef<{ queryTableList: () => void }>();
  const scriptTableRef = useRef<{ queryTableList: () => void }>();

  const { applyProps, setApplyProps } = useOrgStore((s) => ({
    applyProps: s.applyProps,
    setApplyProps: s.setApplyProps,
  }));

  useEffect(() => {
    const { applyType: _applyType, script, dataSourceId, databaseName, schemaName } = applyProps || {};
    console.log({ _applyType, script, dataSourceId, databaseName, schemaName });
    // Handle parameter changes here.
    /**
     * data: data permissions
     * script: script permissions
     * */
    if (_applyType === ApplyType.Data) {
      setIsDrawerVisible(true);
      setApplyType(_applyType);
      dataFormValuesRef.current = {
        dataSourceId: Number(dataSourceId) || undefined,
        databaseName,
        schemaName,
      };
    }
    if (_applyType === ApplyType.Script) {
      setIsDrawerVisible(true);
      setApplyType(_applyType);
      scriptFormValuesRef.current = {
        dataSourceId,
        databaseName,
        schemaName,
        scriptContent: script,
      };
    }
  }, [applyProps]);

  /**
   * Handle a data-permission request.
   */
  const handleApplyDataPermission = async () => {
    if (!curOrg?.id || !dataFormValuesRef.current) {
      return;
    }

    const {
      name,
      description,
      noExpire,
      validUntil,
      isAllSchema,
      dataSourceId,
      databaseInfo,
      rowFilter,
      rowCount,
      policyVOList,
    } = dataFormValuesRef.current;
    const { databaseName, schemaName, tableName, columnNames } = databaseInfo || {};
    const applyDataParams: IApplyDataAccessRequest = {
      organizationId: curOrg?.id,
      name,
      description,
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

    const res = await permissionService.applyDataPermission(applyDataParams);
    if (res) {
      staticMessage.success('申请数据库权限');
      handleCancelDrawer();
      dataTableRef.current?.queryTableList();
    }
  };
  /**
   * Request script permissions.
   */
  const handleApplyScriptPermission = async () => {
    if (!curOrg?.id) return;

    const { name, databaseInfo, description, noExpire, scriptContent, validUntil } = scriptFormValuesRef.current || {};
    const { dataSourceId, databaseName, schemaName } = databaseInfo || {};

    const scriptParams = {
      name,
      organizationId: curOrg?.id,
      dataSourceId,
      databaseName,
      schemaName,
      description,
      noExpire,
      scriptContent,
      validUntil,
    };
    const res = await permissionService.applyScriptPermission(scriptParams);
    if (res) {
      feedback.success('申请脚本权限');
      handleCancelDrawer();
      scriptTableRef.current?.queryTableList();
    }
  };

  /**
   * Query data-permission details.
   */
  const handleQueryDataAccessDetail = async (record: IAccessControlApplyRecordVO) => {
    const res = await permissionService.queryApplyDetail({ id: record.id });
    if (res) {
      const {
        id,
        name,
        description,
        noExpire,
        validUntil,
        columnNames,
        policyVOList,
        dataAccessControlList,
        approvalId,
      } = res;
      const { dataSourceId, databaseName, tableName, schemaName, rowCount, rowFilter } =
        dataAccessControlList?.[0] || {};

      dataFormValuesRef.current = {
        id,
        name,
        description,
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
        approvalId,
      };
      setIsDrawerVisible(true);
    }
  };

  /**
   * Query script-permission details.
   */

  const handleQueryScriptAccessDetail = async (record: IAccessControlApplyRecordVO) => {
    const res = await permissionService.queryApplyDetail({ id: record.id });
    if (res) {
      const {
        id,
        name,
        description,
        noExpire,
        validUntil,
        scriptAccessControlList,
        approvalId,
        status: approvalStatus,
      } = res;
      const { dataSourceId, databaseName, schemaName, scriptContent } = scriptAccessControlList[0] || {};
      scriptFormValuesRef.current = {
        id,
        name,
        description,
        noExpire,
        validUntil,
        scriptContent,
        databaseInfo: {
          dataSourceId,
          databaseName,
          schemaName,
        },
        approvalId,
        approvalStatus,
      };
      setIsDrawerVisible(true);
    }
  };

  const handleCancelDrawer = () => {
    setIsDrawerVisible(false);
    setApplyProps(null);
    dataFormValuesRef.current = undefined;
    scriptFormValuesRef.current = undefined;
  };

  const handleSumbitDrawer = async () => {
    if (applyType === ApplyType.Script) {
      if (scriptFormValuesRef.current?.id) {
        setIsDrawerVisible(false);
        return;
      }
      handleApplyScriptPermission();
    }

    if (applyType === ApplyType.Data) {
      if (dataFormValuesRef.current?.id) {
        setIsDrawerVisible(false);
        return;
      }
      handleApplyDataPermission();
    }
  };

  const renderDrawerTitle = () => {
    if (applyType === ApplyType.Data) {
      return dataFormValuesRef?.current?.id ? i18n('team.apply.viewDataPermission') : i18n('team.apply.applyData');
    } else if (applyType === ApplyType.Script) {
      return scriptFormValuesRef?.current?.id ? '查看脚本权限' : '申请脚本权限';
    }
  };

  return (
    <div className={styles.wrapper}>
      <PageTitle title={i18n('team.nav.permission.apply')} />
      <Tabs
        size="large"
        className={styles.segmented}
        activeKey={applyType}
        items={[
          {
            label: i18n('team.dataPermission'),
            key: ApplyType.Data,
          },
          // {
          //   label: 'Script permissions',
          //   key: ApplyType.Script,
          // },
          // {
          //   label: 'Import permissions',
          //   key: ApplyType.Import,
          // },
        ]}
        onChange={(v) => {
          setApplyType(v as ApplyType);
        }}
      />
      {applyType === ApplyType.Data && (
        <DataTable
          ref={dataTableRef}
          onClickDetail={async (record: IAccessControlApplyRecordVO) => {
            handleQueryDataAccessDetail(record);
          }}
          onClickAdd={() => {
            setIsDrawerVisible(true);
            setApplyType(ApplyType.Data);
          }}
        />
      )}
      {applyType === ApplyType.Script && (
        <ScriptTable
          ref={scriptTableRef}
          onClickDetail={async (record: IAccessControlApplyRecordVO) => {
            handleQueryScriptAccessDetail(record);
          }}
          onClickAdd={() => {
            setIsDrawerVisible(true);
            setApplyType(ApplyType.Script);
          }}
        />
      )}

      {isDrawerVisible && (
        <Drawer
          width={860}
          title={renderDrawerTitle()}
          className={styles.applyDrawer}
          open={isDrawerVisible}
          onClose={handleCancelDrawer}
          extra={
            <Space>
              <Button onClick={handleCancelDrawer}>{i18n('common.button.cancel')}</Button>
              <Button type="primary" onClick={handleSumbitDrawer}>
                {i18n('common.button.submit')}
              </Button>
            </Space>
          }
        >
          {applyType === ApplyType.Data && (
            <DataAccessForm
              type="apply"
              isPreview={!!dataFormValuesRef?.current?.id}
              initData={dataFormValuesRef.current}
              onFormChange={(v) => {
                dataFormValuesRef.current = v;
              }}
            />
          )}

          {applyType === ApplyType.Script && (
            <ScriptForm
              isPreview={!!scriptFormValuesRef?.current?.id}
              initData={scriptFormValuesRef?.current as IAccessControlApplyRecordVO}
              onFormChange={(v) => {
                console.log('script form change', v);
                scriptFormValuesRef.current = {
                  ...v,
                  validUntil: v.noExpire === BooleanType.Yes ? undefined : v.validUntil?.valueOf(),
                };
              }}
            />
          )}
        </Drawer>
      )}
    </div>
  );
}

export default Apply;
