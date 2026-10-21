import { Fragment, memo, useEffect, useState } from 'react';
import { componentMap } from './config/baseConfig';
import { Form } from 'antd';
import { schemaToForm } from './ChartTypeAndDataForm/transform';

const ChartForm = (props) => {
  const [form] = Form.useForm();
  const [formData, setFormData] = useState<any>();
  const { formConfig, chartDetail, onChangeChartSchema } = props;

  const onValuesChange = (values) => {
    const _formData = {
      ...formData,
      ...values,
    };

    setFormData(_formData);
    onChangeChartSchema?.(_formData);
  };

  useEffect(() => {
    const _formData = schemaToForm(chartDetail?.chartSchema);
    form.setFieldsValue(_formData);
    setFormData(_formData);
  }, []);

  return (
    <Form form={form} onValuesChange={onValuesChange} layout="vertical">
      {formConfig?.map((config, index) => (
        <Fragment key={index}>
          <FormItem formData={formData} config={config} chartDetail={chartDetail} />
        </Fragment>
      ))}
    </Form>
  );
};

const FormItem = ({ config, formData, chartDetail }) => {
  const { title, name, valuePropName, component, hidden, components, componentProps = {} } = config;

  if (hidden?.(formData)) {
    return null;
  }

  if (components?.length) {
    return components.map((item, index) => {
      return (
        <Fragment key={index}>
          <FormItem formData={formData} config={item} chartDetail={chartDetail} />
        </Fragment>
      );
    });
  }

  // Get component from map
  const Component = componentMap[component];

  // If the component exists, render it
  return (
    <Form.Item name={name} label={title} valuePropName={valuePropName || 'value'}>
      <Component chartDetail={chartDetail} {...componentProps} />
    </Form.Item>
  );
};

export default memo(ChartForm);
