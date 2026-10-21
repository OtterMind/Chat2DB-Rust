import { memo, useState } from 'react';
import { Input, Button, Form } from 'antd';
import { createStyles } from 'antd-style';
import i18n from '@/i18n';
import { staticMessage } from '@chat2db/ui';

export const useStyles = createStyles(({ css }) => {
  return {
    changePasswordBox: css`
      width: 500px;
      max-width: 70%;
    `,
  };
});

interface IProps {
  updateUser: any;
}

export default memo<IProps>((props) => {
  const { updateUser } = props;
  const [form] = Form.useForm();
  const { styles } = useStyles();
  const [submitLoading, setSubmitLoading] = useState<boolean>(false);

  const onFinish = (values: any) => {
    setSubmitLoading(true);
    if (values.newPassword === values.confirmPassword) {
      updateUser({ password: values.newPassword })
        .then(() => {
          staticMessage.success(i18n('common.message.modifySuccessfully'));
          form.resetFields();
        })
        .finally(() => {
          setSubmitLoading(false);
        });
    } else {
      setSubmitLoading(false);
      staticMessage.error(i18n('login.message.twoPasswordsNotMatch'));
    }
  };

  return (
    <div className={styles.changePasswordBox}>
      <Form form={form} layout="vertical" onFinish={onFinish}>
        <Form.Item
          name="newPassword"
          label={i18n('login.label.newPassword')}
          rules={[
            { required: true, message: i18n('login.tips.requiredPassword') },
            {
              pattern: /^(?=.*[a-z])(?=.*[A-Z])(?=.*\d).{8,}$/,
              message: i18n('login.tips.checkoutPassword'),
            },
          ]}
        >
          <Input.Password placeholder={i18n('login.label.newPassword')} />
        </Form.Item>
        <Form.Item
          name="confirmPassword"
          label={i18n('login.label.confirmPassword')}
          rules={[
            { required: true, message: i18n('login.tips.requiredPassword') },
            {
              pattern: /^(?=.*[a-z])(?=.*[A-Z])(?=.*\d).{8,}$/,
              message: i18n('login.tips.checkoutPassword'),
            },
          ]}
        >
          <Input.Password placeholder={i18n('login.label.confirmPassword')} />
        </Form.Item>
        <Form.Item>
          <Button htmlType="submit" type="primary" loading={submitLoading}>
            {i18n('login.button.savePassword')}
          </Button>
        </Form.Item>
      </Form>
    </div>
  );
});
