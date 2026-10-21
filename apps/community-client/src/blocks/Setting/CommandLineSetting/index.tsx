import { useCallback, useEffect, useState } from 'react';
import { Button, Form, Popconfirm, Progress, Tag, Typography } from 'antd';
import { staticMessage } from '@chat2db/ui';
import i18n from '@/i18n';
import SettingSubsection from '../SettingSubsection';
import { useStyles } from '../BaseSetting/style';

type CliInstallStatus = {
  installed: boolean;
  version?: string | null;
  path?: string | null;
};

type InstallProgress = {
  phase: 'downloading' | 'verifying' | 'installing' | 'installed';
  downloadedBytes: number;
  totalBytes?: number | null;
};

type TauriBridge = {
  core?: { invoke?: (command: string, args?: unknown) => Promise<unknown> };
  event?: {
    listen?: (
      event: string,
      handler: (event: { payload: InstallProgress }) => void,
    ) => Promise<() => void>;
  };
};

function tauriBridge(): TauriBridge | undefined {
  return (globalThis as { __TAURI__?: TauriBridge }).__TAURI__;
}

function phaseLabel(progress: InstallProgress): string {
  switch (progress.phase) {
    case 'verifying':
      return i18n('setting.text.cliPhaseVerifying');
    case 'installing':
      return i18n('setting.text.cliPhaseInstalling');
    case 'installed':
      return i18n('setting.text.cliPhaseInstalled');
    default:
      return i18n('setting.text.cliPhaseDownloading');
  }
}

export default function CommandLineSetting() {
  const { styles } = useStyles();
  const [status, setStatus] = useState<CliInstallStatus>({ installed: false });
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const bridge = tauriBridge();

  const loadStatus = useCallback(async () => {
    const invoke = tauriBridge()?.core?.invoke;
    if (!invoke) {
      return;
    }
    try {
      setStatus((await invoke('cli_status')) as CliInstallStatus);
    } catch {
      setStatus({ installed: false });
    }
  }, []);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  useEffect(() => {
    const listen = tauriBridge()?.event?.listen;
    if (!listen) {
      return undefined;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen('chat2db://cli-install-progress', (event) => {
      setProgress(event.payload);
    })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function install() {
    const invoke = tauriBridge()?.core?.invoke;
    if (!invoke || busy) {
      return;
    }
    setBusy(true);
    setProgress(null);
    try {
      const next = (await invoke('cli_install')) as CliInstallStatus;
      setStatus(next);
      staticMessage.success(i18n('setting.text.cliInstalled'));
    } catch (error) {
      staticMessage.error(`${i18n('setting.text.cliInstallFailed')}: ${String(error)}`);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function uninstall() {
    const invoke = tauriBridge()?.core?.invoke;
    if (!invoke || busy) {
      return;
    }
    setBusy(true);
    try {
      setStatus((await invoke('cli_uninstall')) as CliInstallStatus);
      staticMessage.success(i18n('setting.text.cliRemoved'));
    } catch (error) {
      staticMessage.error(`${i18n('setting.text.cliRemoveFailed')}: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  }

  if (!bridge?.core?.invoke) {
    return null;
  }

  const percent =
    progress?.totalBytes && progress.totalBytes > 0
      ? Math.min(100, Math.round((progress.downloadedBytes / progress.totalBytes) * 100))
      : undefined;

  return (
    <div className={styles.baseSettingBox}>
      <div data-setting-search-id="commandLine.status">
        <SettingSubsection
          title={<span data-setting-search-title="true">{i18n('setting.title.commandLine')}</span>}
          describe={i18n('setting.text.commandLineDescribe')}
        />
        <Form className={styles.customFontBox} layout="vertical">
          <div>
            {status.installed ? (
              <Tag color="green">{i18n('setting.text.cliInstalledTag')}</Tag>
            ) : (
              <Tag>{i18n('setting.text.cliNotInstalledTag')}</Tag>
            )}
            {status.installed && status.version ? (
              <Typography.Text type="secondary" style={{ marginLeft: 8 }}>
                {i18n('setting.text.cliInstalledVersion')} {status.version}
              </Typography.Text>
            ) : null}
          </div>
          {status.installed && status.path ? (
            <Typography.Text type="secondary" copyable={{ text: status.path }}>
              {status.path}
            </Typography.Text>
          ) : null}
          {progress ? (
            <div>
              <Typography.Text type="secondary">{phaseLabel(progress)}</Typography.Text>
              <Progress percent={percent} status="active" showInfo={percent !== undefined} />
            </div>
          ) : null}
          <div>
            <Button type="primary" loading={busy} onClick={() => void install()}>
              {status.installed ? i18n('setting.button.reinstallCli') : i18n('setting.button.installCli')}
            </Button>
            {status.installed ? (
              <Popconfirm
                title={i18n('setting.text.cliUninstallConfirm')}
                onConfirm={() => void uninstall()}
              >
                <Button danger disabled={busy} style={{ marginLeft: 8 }}>
                  {i18n('setting.button.uninstallCli')}
                </Button>
              </Popconfirm>
            ) : null}
          </div>
        </Form>
      </div>
    </div>
  );
}
