import { FC, useEffect, useMemo, useState } from 'react';
import { Form, Input, InputNumber, ModalProps, Select } from 'antd';
import { Modal } from '@chat2db/ui';
import i18n from '@/i18n';
import { useDashboardStore } from '@/store/dashboard/store';
import ModalTitle from '@/components/Modal/ModalTitle';
import { AUTO_REFRESH, AUTO_REFRESH_DASHBOARD_OPTIONS } from '@/blocks/BI/Chart/constants';

export interface DashboardSettingProps extends ModalProps {}

const DashboardSetting: FC<DashboardSettingProps> = () => {
  const { handleConfirmSettingModal, settingDashboard, setSettingDashboard } = useDashboardStore((state) => ({
    settingDashboard: state.settingDashboard,
    handleConfirmSettingModal: state.handleConfirmSettingModal,
    setSettingDashboard: state.setSettingDashboard,
  }));
  const [form] = Form.useForm();
  const [formData, setFormData] = useState<any>({});

  useEffect(() => {
    if (!settingDashboard) {
      form.resetFields();
      setFormData({});
      return;
    }
    const { id, name, description, refreshCycle, refreshType } = settingDashboard || {};
    const _formData = {
      id,
      name,
      description,
      refreshCycle,
      refreshType: refreshType || AUTO_REFRESH.NEVER,
    };
    form.setFieldsValue(_formData);
    setFormData(_formData);
  }, [settingDashboard]);

  const handleOk = () => {
    form.validateFields().then((values) => {
      handleConfirmSettingModal({
        id: settingDashboard?.id,
        ...values,
      });
    });
  };

  const handleCancel = () => {
    setSettingDashboard(undefined);
  };

  const autoRefreshOptions = useMemo(() => {
    return AUTO_REFRESH_DASHBOARD_OPTIONS.map((item) => ({
      ...item,
      label: i18n(item.label as any),
    }));
  }, []);

  return (
    <Modal
      title={
        <ModalTitle
          iconCode="icon-chart-square-bar"
          title={settingDashboard?.id ? i18n('dashboard.modal.editTitle') : i18n('dashboard.modal.addTitle')}
        />
      }
      maskClosable={false}
      open={!!settingDashboard}
      onOk={handleOk}
      onCancel={handleCancel}
      okText={i18n('common.button.confirm')}
      cancelText={i18n('common.button.cancel')}
      destroyOnClose={true}
      width={520}
    >
      <Form
        form={form}
        layout="vertical"
        autoComplete={'off'}
        onValuesChange={(changedValues, allValues) => {
          setFormData(allValues);
        }}
      >
        <Form.Item
          label={i18n('common.label.name')}
          name={'name'}
          rules={[{ required: true, message: i18n('dashboard.createName.placeholder') }]}
        >
          <Input placeholder={i18n('dashboard.createName.placeholder')} />
        </Form.Item>
        <Form.Item label={i18n('common.label.description')} name={'description'}>
          <Input placeholder={i18n('dashboard.createDescription.placeholder')} />
        </Form.Item>
        <Form.Item name="refreshType" label={i18n('dashboard.refresh.rule')}>
          <Select options={autoRefreshOptions} />
        </Form.Item>
        {formData?.refreshType === AUTO_REFRESH.MINUTES && (
          <Form.Item name="refreshCycle" label={i18n('dashboard.refresh.minutes.refresh')}>
            <InputNumber
              min={1}
              style={{ width: '100%' }}
              placeholder={i18n('dashboard.refresh.minutes.refresh.placeholder')}
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  );
};

export default DashboardSetting;
