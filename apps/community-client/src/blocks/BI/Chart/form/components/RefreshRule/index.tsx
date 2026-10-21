/*
 * ThemeColorSelect
 * report theme color selector
 */
import { memo, useEffect } from 'react';
// import { useStyles } from './style';
import { Form, InputNumber, Select, TimePicker } from 'antd';
import i18n from '@/i18n';
import { AUTO_REFRESH, AUTO_REFRESH_OPTIONS } from '../../../constants';
import dayjs from 'dayjs';

interface IProps {
  value: any;
  onChange?: (value: any) => void;
}

const AutoRefreshSelect = (props: IProps) => {
  const { value, onChange } = props;

  const [form] = Form.useForm();

  useEffect(() => {
    form.setFieldsValue({
      ...value,
      everydayRefresh: dayjs(value?.everydayRefresh, 'HH:mm'),
    });
  }, [value]);

  const handleOnValuesChange = () => {
    const data = form.getFieldsValue();

    if (data?.everydayRefresh) {
      data.everydayRefresh = dayjs(data?.everydayRefresh).format('HH:mm');
    }

    onChange?.({
      ...value,
      ...data,
    });
  };

  const autoRefreshOptions = AUTO_REFRESH_OPTIONS.map((item) => ({
    ...item,
    label: i18n(item.label as any),
  }));

  return (
    <Form form={form} layout="vertical" onValuesChange={handleOnValuesChange}>
      <Form.Item name="refreshRule" label={i18n('dashboard.refresh.rule')}>
        <Select options={autoRefreshOptions} />
      </Form.Item>
      {value?.refreshRule === AUTO_REFRESH.EVERYDAY && (
        <Form.Item name="everydayRefresh" label={i18n('dashboard.refresh.everyday')}>
          <TimePicker format="HH:mm" style={{ width: '100%' }} changeOnScroll needConfirm={false} />
        </Form.Item>
      )}
      {value?.refreshRule === AUTO_REFRESH.MINUTES && (
        <Form.Item name="minutesRefresh" label={i18n('dashboard.refresh.minutes')}>
          <InputNumber style={{ width: '100%' }} />
        </Form.Item>
      )}
      {value?.refreshRule === AUTO_REFRESH.CRON && (
        <Form.Item name="cronRefresh" label={i18n('dashboard.refresh.minutes')}>
          <InputNumber style={{ width: '100%' }} />
        </Form.Item>
      )}
    </Form>
  );
};

export default memo(AutoRefreshSelect);
