import { FC, useMemo, useState } from 'react';
import { CopyButton, Input, Modal, ModalProps, Select } from '@chat2db/ui';
import { ChatVO } from '@/typings/chat';
import { Button, Form, Spin } from 'antd';
import i18n from '@/i18n';
export interface ChatShareProps extends ModalProps {
  chatItem?: ChatVO;
}

const scopeOptions = [
  {
    label: i18n('chat.share.scope.all'),
    value: 'all',
  },
  {
    label: i18n('chat.share.scope.collaborator'),
    value: 'collaborator',
    disabled: true,
  },
];

const permissionOptions = [
  {
    label: i18n('chat.share.permission.view'),
    value: 'view',
  },
  {
    label: i18n('chat.share.permission.edit'),
    value: 'edit',
  },
];

const preUrl = location.origin + '/chat?share_id=';

const ChatShare: FC<ChatShareProps> = ({ chatItem, open, onCancel, onOk }) => {
  const { editShareId, viewShareId } = chatItem || {};
  const [loading, setLoading] = useState(false);
  const [form] = Form.useForm();
  const shareLink = Form.useWatch('shareLink', form);

  const viewShareLink = useMemo(() => preUrl + viewShareId + '&share_type=view', [viewShareId]);
  const editShareLink = useMemo(() => preUrl + editShareId + '&share_type=edit', [editShareId]);

  return (
    <Modal
      title={`Share [${chatItem?.title || i18n('chat.noTitle')}]`}
      open={open}
      onCancel={onCancel}
      maskClosable={false}
      footer={
        <Button type="primary" onClick={onOk}>
          {i18n('common.button.affirm')}
        </Button>
      }
    >
      <Form
        form={form}
        initialValues={{
          scope: 'all',
          permission: 'view',
          shareLink: viewShareLink,
        }}
        onValuesChange={({ permission }) => {
          setLoading(true);
          setTimeout(() => {
            if (permission === 'edit') {
              form.setFieldValue('shareLink', editShareLink);
            } else {
              form.setFieldValue('shareLink', viewShareLink);
            }
            setLoading(false);
          }, 1000);
        }}
      >
        <Form.Item name={'scope'} label={i18n('chat.share.scope')}>
          <Select options={scopeOptions} />
        </Form.Item>
        <Form.Item name={'permission'} label={i18n('chat.share.permission')}>
          <Select options={permissionOptions} />
        </Form.Item>
        <Spin style={{ display: 'block' }} spinning={loading}>
          <Form.Item name={'shareLink'} label={i18n('chat.share.link')}>
            <Input
              disabled
              suffix={
                <CopyButton
                  size="xs"
                  copySuccessText={i18n('common.button.copySuccessfully')}
                  copyContent={shareLink}
                />
              }
            />
          </Form.Item>
        </Spin>
      </Form>
    </Modal>
  );
};

export default ChatShare;
