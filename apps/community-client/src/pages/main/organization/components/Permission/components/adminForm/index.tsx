import React, { useEffect, useMemo, useState } from 'react';
import { DatePicker, Form, Input, Radio, Select } from 'antd';
import orgService from '@/service/enterprise/organization';
import connectionServie from '@/service/connection';
import { useOrgStore } from '@/store/organization';
import { debounce } from 'lodash';
import dayjs from 'dayjs';
import { BooleanType } from '@/typings/common';
import { databaseMap } from '@/constants';
import styles from './index.less';
import i18n from '@/i18n';
import { IconfontSvg } from '@chat2db/ui';

interface IOption {
  label: string | React.ReactNode;
  value: number | string;
}
export interface IAdminFrom {
  id?: number;
  name: string;
  description: string;
  userId: number | { label: string; value: number };
  dataSourceId: number | { label: string; value: number };
  noExpire: BooleanType;
  validUntil?: number;
}

interface IAdminFormProps {
  isPreview?: boolean;
  initData?: IAdminFrom;
  onAuthFormChange: (values: IAdminFrom) => void;
}

function AdminForm(props: IAdminFormProps) {
  const { onAuthFormChange, isPreview } = props;
  const [form] = Form.useForm();

  const [userOptions, setUserOptions] = useState<IOption[]>([]);
  const [userFetching, setUserFetching] = useState(false);
  const [dataSourceOptions, setDataSourceOptions] = useState<IOption[]>([]);
  const [dataSourceFetching, setDataSourceFetching] = useState(false);
  const isNoExpire = Form.useWatch('noExpire', form);
  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));

  useEffect(() => {
    if (isPreview) {
      form.setFieldsValue({
        ...props.initData,
        validUntil: props.initData?.validUntil ? dayjs(props.initData?.validUntil) : undefined,
      });
    } else {
      handleSearchUser('');
      handleSearchDataSource('');
      form.resetFields();
      form.setFieldsValue({
        noExpire: BooleanType.Yes,
      });
    }
  }, []);

  /**
   * Search for users.
   */
  const handleSearchUser = async (value: string) => {
    if (!curOrg?.id) return;
    setUserOptions([]);
    setUserFetching(true);

    const res = await orgService.getOrganizationUserList({ organizationId: curOrg?.id, searchKey: value });
    const options = (res?.data ?? []).map((item) => ({ label: item.displayName, value: item.id }));

    setUserOptions(options);
    setUserFetching(false);
  };
  /**
   * Debounced user search.
   */
  const debounceSearchUser = useMemo(() => debounce(handleSearchUser, 300), []);

  /**
   * Search for data sources.
   */
  const handleSearchDataSource = async (value: string) => {
    setDataSourceOptions([]);
    setDataSourceFetching(true);
    const res = await connectionServie.getList({
      searchKey: value,
      pageNo: 1,
      pageSize: 50,
    });
    const options = (res?.data ?? []).map((item) => ({
      key: item.id,
      value: item.id,
      label: (
        <div style={{ display: 'flex', alignItems: 'center' }}>
          <IconfontSvg code={databaseMap[item.type!]?.icon} />
          <div style={{ marginLeft: '8px' }}>{item.alias}</div>
        </div>
      ),
    }));

    setDataSourceOptions(options);
    setDataSourceFetching(false);
  };
  /**
   * Debounced data-source search.
   */
  const debounceSearchDataSource = useMemo(() => debounce(handleSearchDataSource, 300), []);

  /**
   * Form onChange handler.
   */
  const handleFormChange = (v, allValues) => {
    if (isPreview) return;

    const { validUntil } = allValues;
    onAuthFormChange &&
      onAuthFormChange({
        ...allValues,
        validUntil: validUntil ? dayjs(validUntil).valueOf() : undefined,
      });
  };

  return (
    <Form
      labelCol={{
        span: 4,
      }}
      className={styles.formWrapper}
      form={form}
      onValuesChange={handleFormChange}
      autoComplete="off"
      disabled={isPreview}
    >
      <Form.Item label={i18n('team.drawer.name')} name="name">
        <Input />
      </Form.Item>
      <Form.Item label={i18n('team.drawer.desc')} name="description">
        <Input.TextArea maxLength={250} />
      </Form.Item>
      <Form.Item label={i18n('team.drawer.user')} name="userId">
        <Select
          filterOption={false}
          loading={userFetching}
          options={userOptions}
          showSearch
          onSearch={debounceSearchUser}
        />
      </Form.Item>
      <Form.Item label={i18n('team.drawer.datasource')} name="dataSourceId">
        <Select
          showSearch
          filterOption={false}
          loading={dataSourceFetching}
          options={dataSourceOptions}
          onSearch={debounceSearchDataSource}
        />
      </Form.Item>

      <Form.Item label={i18n('team.drawer.isPermanent')} name="noExpire">
        <Radio.Group>
          <Radio value={BooleanType.Yes}>{i18n('common.text.is')}</Radio>
          <Radio value={BooleanType.No}>{i18n('common.text.no')}</Radio>
        </Radio.Group>
      </Form.Item>
      {isNoExpire === BooleanType.Yes ? null : (
        <Form.Item label={i18n('team.drawer.expire')} name="validUntil">
          <DatePicker showTime disabledDate={(current) => current < dayjs().startOf('day')} />
        </Form.Item>
      )}
    </Form>
  );
}

export default AdminForm;
