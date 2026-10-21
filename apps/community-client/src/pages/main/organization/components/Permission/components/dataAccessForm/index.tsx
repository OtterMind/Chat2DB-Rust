import { DatePicker, Drawer, Form, Input, Select, Button, Tag, Flex } from 'antd';
import React, { useEffect, useMemo, useState } from 'react';
import { debounce } from 'lodash';
import { useOrgStore } from '@/store/organization';
import orgService from '@/service/enterprise/organization';
import connectionService from '@/service/connection';
import dayjs from 'dayjs';
import { databaseMap } from '@/constants';

import { BooleanType } from '@/typings/common';
import ApprovalFlow from '@/pages/main/organization/components/Approval/approvalFlow';
import { useStyles } from './style';
import i18n from '@/i18n';
import { IconfontSvg } from '@chat2db/ui';

interface IOption {
  label: string | React.ReactNode;
  value: number | string;
}
/**
 * Data-permission form data structure.
 */
export interface IDataAccessForm {
  id?: number;
  name: string;
  description: string;
  userId?: number;
  dataSourceId: number;
  isAllSchema: boolean;
  databaseInfo: {
    dataSourceId?: number;
    databaseName: string;
    schemaName: string;
    tableName: string;
    columnNames: string[];
  };
  noExpire: BooleanType;
  validUntil?: number;
  policyVOList: string[];
  rowFilter?: string;
  rowCount?: number;
  approvalId?: number;
}

interface IProps {
  type?: 'auth' | 'apply'; // auth: grant; apply: request.
  isPreview?: boolean;
  initData?: any;
  onFormChange?: (values) => void;
  showApprovalFlowBtn?: boolean;
}

function DataAccessForm(props: IProps) {
  const { styles } = useStyles();
  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));
  const { onFormChange, initData, isPreview, showApprovalFlowBtn = true } = props;
  const [userOptions, setUserOptions] = useState<{ label: string; value: number }[]>([]);
  const [userFetching, setUserFetching] = useState(false);
  const [dataSourceOptions, setDataSourceOptions] = useState<IOption[]>([]);
  const [openWorkflow, setOpenWorkflow] = useState(false);

  const [form] = Form.useForm();
  useEffect(() => {
    initializePolicyList();
    handleSearchUser('');
    handleSearchDataSource('');
    if (isPreview) {
      form.setFieldsValue({
        ...initData,
        validUntil: props.initData?.validUntil ? dayjs(props.initData?.validUntil) : undefined,
      });
    } else {
      form.setFieldsValue({
        applyType: 'ADMIN',
        isAllSchema: true,
        noExpire: BooleanType.Yes,
        ...props.initData,
      });
    }
  }, []);

  const initializePolicyList = () => {
    const initPolicy = form.getFieldValue('policyVOList');
    form.setFieldValue('policyVOList', initPolicy ? initPolicy : ['SELECT']);
  };

  const handleSearchUser = async (value: string) => {
    if (!curOrg?.id) return;
    setUserOptions([]);
    setUserFetching(true);

    const res = await orgService.getOrganizationUserList({ organizationId: curOrg?.id, searchKey: value });
    const options = (res?.data ?? []).map((item) => ({ label: item.displayName, value: item.id }));

    setUserOptions(options);
    setUserFetching(false);
  };

  const debounceSearchUser = useMemo(() => debounce(handleSearchUser, 300), []);

  const handleSearchDataSource = async (value?: string) => {
    setDataSourceOptions([]);

    const res = await connectionService.getList({
      searchKey: value,
      pageNo: 1,
      pageSize: 50,
    });
    const options = (res?.data ?? []).map((item) => ({
      key: item.id,
      value: item.id,
      label: (
        <Flex align="center" gap={4}>
          <IconfontSvg code={databaseMap[item.type!]?.icon} />
          <div style={{ marginLeft: '8px' }}>{item.alias}</div>
          <Tag color={item.environment.shortName === 'TEST' ? 'volcano' : 'lime'}>{item.environment.name}</Tag>
        </Flex>
      ),
    }));

    setDataSourceOptions(options);
  };
  const debounceSearchDataSource = useMemo(() => debounce(handleSearchDataSource, 300), []);

  const handleFormChange = (v, allValues) => {
    if (isPreview) return;

    const { validUntil } = allValues;
    let vTemp = allValues;
    if (allValues['databaseInfo']) {
      const { databaseName, schemaName, tableName, columnNames } = allValues['databaseInfo'];

      vTemp = {
        ...vTemp,
        databaseName,
        schemaName,
        tableName,
        columnNames: (columnNames || []).includes('all') ? [] : columnNames,
      };
    }

    onFormChange &&
      onFormChange({
        ...vTemp,
        validUntil: validUntil ? dayjs(validUntil).valueOf() : undefined,
      });
  };

  const renderApprovalFlow = () => {
    const { approvalId } = initData || {};

    if (!approvalId) return null;

    if (!showApprovalFlowBtn) return null;

    return (
      <>
        <Button type="primary" onClick={() => setOpenWorkflow(true)} style={{ marginLeft: '32px' }}>
          {i18n('team.approval.flow.title')}
        </Button>
        <Drawer
          title={i18n('team.approval.flow.title')}
          open={openWorkflow}
          width={720}
          onClose={() => setOpenWorkflow(false)}
        >
          <ApprovalFlow approvalId={approvalId} />
        </Drawer>
      </>
    );
  };

  return (
    <>
      <Form
        layout="vertical"
        // labelCol={{ span: 4 }}
        className={styles.formWrapper}
        form={form}
        onValuesChange={handleFormChange}
        autoComplete="off"
        disabled={isPreview}
      >
        <Form.Item
          label={i18n('team.drawer.name')}
          name="name"
          rules={[
            {
              required: true,
              // message: 'Required',
            },
          ]}
        >
          <Input />
        </Form.Item>
        <Form.Item
          label={i18n('team.drawer.desc')}
          name="description"
          rules={[
            {
              required: true,
              // message: 'Required',
            },
          ]}
        >
          <Input.TextArea maxLength={250} />
        </Form.Item>
        {props.type === 'auth' ? (
          <Form.Item label={i18n('team.drawer.user')} name="userId" rules={[{ required: true }]}>
            <Select
              filterOption={false}
              loading={userFetching}
              // notFoundContent={
              //   fetching ? <Spin style={{ display: 'flex', justifyContent: 'center', padding: '20px 0' }} /> : null
              // }
              options={userOptions}
              onSearch={debounceSearchUser}
              showSearch
            />
          </Form.Item>
        ) : null}

        <Form.Item label={i18n('team.drawer.datasource')} name="dataSourceId" rules={[{ required: true }]}>
          <Select
            showSearch
            filterOption={false}
            // notFoundContent={
            //   fetching ? <Spin style={{ display: 'flex', justifyContent: 'center', padding: '20px 0' }} /> : null
            // }
            options={dataSourceOptions}
            onSearch={debounceSearchDataSource}
          />
        </Form.Item>
        {/* <Form.Item label="Full database access" name="isAllSchema">
          <Radio.Group>
            <Radio value={true}>Yes</Radio>
            <Radio value={false}>No</Radio>
          </Radio.Group>
        </Form.Item> */}
        {/* {isAllSchema || !dataSourceId ? null : (
          <>
            <div>
              <Form.Item label="Permission details" name="databaseInfo">
                <CascaderDB
                  initData={{
                    ...initData?.databaseInfo,
                    ...(initData?.dataSourceId ? { dataSourceId: initData?.dataSourceId } : {}),
                  }}
                  notShowList={['datasource']}
                  dataSourceId={dataSourceId}
                  dataSourceOptions={dataSourceOptions}
                />
              </Form.Item>
            </div>
          </>
        )} */}

        {/* <Form.Item label="Never expires" name="noExpire">
          <Radio.Group>
            <Radio value={BooleanType.Yes}>Yes</Radio>
            <Radio value={BooleanType.No}>No</Radio>
          </Radio.Group>
        </Form.Item> */}

        {/* <Form.Item label="Permission scope" name="policyVOList" rules={[{ required: true, message: 'Required' }]}>
          <TreeSelect
            multiple
            allowClear
            showSearch
            treeCheckable
            treeData={policyList}
            treeDefaultExpandedKeys={['DML']}
          />
        </Form.Item> */}
        {/* <Form.Item label="Rows visible per request" name="rowCount">
          <InputNumber
            min={1}
            max={10000000}
            step={5}
            formatter={(value) => (value ? String(parseInt(value, 10)) : '')}
          />
        </Form.Item> */}
        {/* <Form.Item label="Row filter" name="rowFilter">
          <Input placeholder='Example: "id > 100 and id < 200"' />
        </Form.Item> */}

        {/* {isNoExpire === BooleanType.Yes ? null : (
          <Form.Item label="Permission expiration time" name="validUntil">
            <DatePicker showTime disabledDate={(current) => current < dayjs().startOf('day')} />
          </Form.Item>
        )} */}

        <Form.Item label={i18n('team.drawer.expire')} name="validUntil" required>
          <DatePicker showTime disabledDate={(current) => current < dayjs().startOf('day')} />
        </Form.Item>
      </Form>
      {renderApprovalFlow()}
    </>
  );
}

export default DataAccessForm;
