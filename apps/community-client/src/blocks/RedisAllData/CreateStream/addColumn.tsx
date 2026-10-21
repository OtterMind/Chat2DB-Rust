// Pinned form
import { useState, useRef, useEffect } from 'react';
import { Button, Input } from 'antd';
import i18n from '@/i18n';
import { openModal } from '@/store/common/components';
import { createStyles, css } from 'antd-style';

export const useStyles = createStyles(({ token }) => {
  return {
    modalContent: css`
      padding-top: 20px;
      .ant-input-affix-wrapper-lg {
        height: 42px;
      }
    `,
    prefixIcon: css`
      color: ${token.colorTextTertiary};
    `,
    footer: css`
      display: flex;
      justify-content: flex-end;
      margin-top: 24px;
      gap: 8px;
    `,
  };
});

export const openAddColumnModal = (loadData) => {
  openModal({
    width: '600px',
    title: i18n('redis.button.addColumnName'),
    content: <AddColumnContent loadData={loadData} openModal={openModal} />,
  });
};

export const AddColumnContent = (params: { openModal: any; loadData: any }) => {
  const { loadData } = params;
  const [value, setValue] = useState('');
  const { styles } = useStyles();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.focus();
    }
  }, []);

  const onOk = () => {
    if (!value) {
      return;
    }
    loadData(value);
    openModal(false);
  };

  const handleChange = (e) => {
    setValue(e.target.value);
  };

  return (
    <div className={styles.modalContent}>
      <Input size="large" onChange={handleChange} value={value} ref={inputRef} />
      <div className={styles.footer}>
        <Button
          onClick={() => {
            openModal(false);
          }}
        >
          {i18n('common.button.cancel')}
        </Button>
        <Button type="primary" onClick={onOk}>
          {i18n('common.button.affirm')}
        </Button>
      </div>
    </div>
  );
};
