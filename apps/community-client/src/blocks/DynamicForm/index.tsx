import { forwardRef, useImperativeHandle, ForwardedRef } from 'react';
import { Form, Input, Select, Checkbox } from 'antd';
import { IDynamicFormItem } from './typings';
import { InputType } from './constants';
import { useStyles } from './style';

export interface DynamicFormRef {
  getFieldsValue: () => Record<string, any>;
  setFieldsValue: (values: Record<string, any>) => void;
  resetFields: () => void;
}

interface DynamicFormProps {
  config: IDynamicFormItem[];
  onFinish?: (values: any) => void;
  initialValues?: Record<string, any>;
}

const DynamicForm = forwardRef(
  ({ config, onFinish, initialValues }: DynamicFormProps, ref: ForwardedRef<DynamicFormRef>) => {
    const [form] = Form.useForm();
    const { styles, cx } = useStyles();

    useImperativeHandle(ref, () => ({
      getFieldsValue: () => form.getFieldsValue(),
      setFieldsValue: (values) => form.setFieldsValue(values),
      resetFields: () => form.resetFields(),
    }));

    const renderFormItem = (item: IDynamicFormItem) => {
      const { inputType, multiple, selects = [] } = item;

      switch (inputType) {
        case InputType.INPUT:
          return <Input />;
        case InputType.SELECT:
          return (
            <Select
              mode={multiple ? 'multiple' : undefined}
              size="small"
              popupClassName={styles.dropdown}
              options={selects.map((opt) => ({
                value: opt.value,
                label: opt.label ?? '',
              }))}
              optionRender={(option) => {
                return <span>{option.label ?? ''}</span>;
              }}
            />
          );
        case InputType.CHECKBOX:
          if (multiple) {
            return (
              <Checkbox.Group>
                {selects.map((opt) => (
                  <Checkbox key={opt.value} value={opt.value}>
                    {opt.label ?? ''}
                  </Checkbox>
                ))}
              </Checkbox.Group>
            );
          }
          return <Checkbox>{item.labelName}</Checkbox>;
        default:
          return null;
      }
    };

    const renderLabel = (item: IDynamicFormItem) => {
      // If the inputType is checkbox, the label will not be displayed.
      if (item.inputType === InputType.CHECKBOX) {
        return null;
      }
      return item.labelName;
    };

    const valuePropName = (item: IDynamicFormItem) => {
      if (item.inputType === InputType.CHECKBOX) {
        return 'checked';
      }
      return undefined;
    };

    return (
      <div className={cx(styles.container, styles.smallForm)}>
        <Form form={form} layout="vertical" onFinish={onFinish} initialValues={initialValues} size="small">
          {config.map((item) => (
            <Form.Item
              key={item.name}
              name={item.name}
              label={renderLabel(item)}
              rules={[
                {
                  required: item.required,
                  message: `请输入${item.labelName}`,
                },
              ]}
              hidden={item.display === 'false'}
              valuePropName={valuePropName(item)}
            >
              {renderFormItem(item)}
            </Form.Item>
          ))}
        </Form>
      </div>
    );
  },
);

export default DynamicForm;
