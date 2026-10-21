import { memo, useState } from 'react';
import { Input, Button, Form } from 'antd';
import Avatar from '@/components/Avatar';
import { createStyles } from 'antd-style';
import i18n from '@/i18n';
import { staticMessage } from '@chat2db/ui';

export const useStyles = createStyles(({ css, token }) => {
  return {
    personalBox: css`
      display: flex;
      flex-direction: column;
      gap: 30px;
      width: 500px;
      max-width: 70%;
    `,
    avatarBox: css`
      display: flex;
      gap: 20px;
      align-items: center;
    `,
    avatar: css`
      background-color: ${token.colorPrimaryBg};
      color: ${token.colorPrimary};
    `,
    changeAvatarTips: css`
      font-size: 12px;
      color: ${token.colorTextSecondary};
      margin-top: 12px;
    `,
  };
});

interface IProps {
  curUser: any;
  updateUser: any;
}

export default memo<IProps>((props) => {
  const { curUser, updateUser } = props;
  const { styles } = useStyles();
  const [form] = Form.useForm();
  const [submitLoading, setSubmitLoading] = useState<boolean>(false);

  const onFinish = (values: any) => {
    setSubmitLoading(true);
    updateUser({ displayName: values.displayName })
      .then(() => {
        staticMessage.success(i18n('common.tips.updateSuccess'));
      })
      .finally(() => {
        setSubmitLoading(false);
      });
  };

  return (
    <div className={styles.personalBox}>
      <div className={styles.avatarBox}>
        <Avatar size={96} canEditor={true} />
        <div>
          <Avatar canEditor={true}>
            <Button type="primary">{i18n('setting.button.changeAvatar')}</Button>
          </Avatar>
          <div className={styles.changeAvatarTips}>{i18n('setting.update.tip')}</div>
        </div>
      </div>
      <div>
        {/* Display name. */}
        <Form form={form} layout="vertical" onFinish={onFinish} initialValues={{ displayName: curUser.displayName }}>
          <Form.Item name="displayName" label={i18n('setting.label.displayName')}>
            <Input placeholder={i18n('setting.label.displayName')} />
          </Form.Item>
          <Form.Item>
            <Button htmlType="submit" type="primary" loading={submitLoading}>
              {i18n('setting.label.updateName')}
            </Button>
          </Form.Item>
        </Form>
      </div>
    </div>
  );
});
