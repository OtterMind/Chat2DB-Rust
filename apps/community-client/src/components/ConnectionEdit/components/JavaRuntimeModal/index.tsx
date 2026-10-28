import { Alert, Button, Modal, Progress, Typography } from 'antd';
import { useEffect, useState } from 'react';
import { i18n } from '@/i18n';
import { installJavaRuntime } from '../../utils/javaRuntime';

interface Props {
  open: boolean;
  onCancel: () => void;
  onReady: () => void;
}

export default function JavaRuntimeModal({ open, onCancel, onReady }: Props) {
  const [installing, setInstalling] = useState(false);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    setInstalling(false);
    setDownloaded(0);
    setTotal(null);
    setError(null);
  }, [open]);

  async function install() {
    setInstalling(true);
    setError(null);
    try {
      await installJavaRuntime((progress) => {
        setDownloaded(progress.downloadedBytes);
        setTotal(progress.totalBytes);
      });
      onReady();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setInstalling(false);
    }
  }

  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : undefined;

  return (
    <Modal
      open={open}
      title={i18n('connection.javaRuntime.title')}
      onCancel={installing ? undefined : onCancel}
      footer={[
        <Button key="later" onClick={onCancel} disabled={installing}>
          {i18n('common.button.cancel')}
        </Button>,
        <Button key="install" type="primary" loading={installing} onClick={install}>
          {i18n('connection.javaRuntime.install')}
        </Button>,
      ]}
    >
      <Typography.Paragraph>{i18n('connection.javaRuntime.description')}</Typography.Paragraph>
      {installing && <Progress percent={percent} status="active" />}
      {error && (
        <Alert
          type="error"
          showIcon
          message={i18n('connection.javaRuntime.failed')}
          description={error}
        />
      )}
    </Modal>
  );
}
