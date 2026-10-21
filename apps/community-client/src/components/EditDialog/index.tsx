import { memo, useEffect, useRef, useState } from 'react';
import styles from './index.less';
import classnames from 'classnames';
import MonacoEditor, { IRangeType } from '@/components/MonacoEditor';
import { Modal } from 'antd';

interface IProps {
  className?: string;
  verifyDialog: boolean;
  title: string;
  value: {
    text: string;
    range: IRangeType;
  };
}

export default memo<IProps>((props) => {
  const { className, verifyDialog, value, title } = props;
  const [open, setOpen] = useState<boolean>();
  const monacoEditorRef = useRef<any>();

  useEffect(() => {
    setOpen(verifyDialog);
  }, [verifyDialog]);

  useEffect(() => {
    if (monacoEditorRef.current) {
      monacoEditorRef.current?.setValue(value.text, value.range);
    }
  }, [value]);

  return (
    <div className={classnames(styles.box, className)}>
      <Modal
        title={title}
        open={open}
        width={800}
        maskClosable={false}

        // onCancel={(() => { setVerifyDialog(false) })}
      >
        <MonacoEditor id="edit-dialog" ref={monacoEditorRef} />
      </Modal>
    </div>
  );
});
