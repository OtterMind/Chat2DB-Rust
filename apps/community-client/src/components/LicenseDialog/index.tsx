import { ModalProps, staticMessage } from '@chat2db/ui';
import { useEffect, useState } from 'react';
import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import LicenseService from '@/service/license';
import i18n from '@/i18n';
import ActivationDialog from './ActivationDialog';
import UnbindDialog from './UnbindDialog';
import DoubleCheckDialog from './DoubleCheckDialog';
import { LicenseDialogType } from '../GuideDialog/type';
import { openWebPage } from '@/utils/url';

interface LicenseDialogProps extends ModalProps {}

const LicenseDialog = (props: LicenseDialogProps) => {
  const { open, ...rest } = props;
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [license, setLicense] = useState<string>('');
  const [activationError, setActivationError] = useState<string | null>(null);
  const [activationInfo, setActivationInfo] = useState<any>(null);

  const { setOpenLinenseDialog, setOpenGuideDialog, appUrlConfig, licenseDialogType, setLicenseDialogType } =
    useGlobalStore((state) => ({
      setOpenLinenseDialog: state.setOpenLinenseDialog,
      setOpenGuideDialog: state.setOpenGuideDialog,
      licenseDialogType: state.licenseDialogType,
      setLicenseDialogType: state.setLicenseDialogType,
      appUrlConfig: state.appUrlConfig,
    }));

  const { curUser, queryCurUser } = useUserStore((s) => ({
    curUser: s.curUser,
    queryCurUser: s.queryCurUser,
  }));

  useEffect(() => {
    if (open) {
      resetState();
      queryCurUser();
    }
  }, [open]);

  useEffect(() => {
    if (!curUser) {
      setOpenGuideDialog(true);
    }
  }, [curUser]);

  const resetState = () => {
    setLicense('');
    setActivationError(null);
  };

  const handleClose = () => {
    setOpenLinenseDialog(false);
    setLicenseDialogType(null);
    resetState();
    setActivationInfo(null);
  };

  const handleSuccess = async () => {
    await queryCurUser();
    setOpenGuideDialog(false);
    handleClose();
  };

  const handleError = (errorMessage: string) => {
    setActivationError(errorMessage);
  };

  const handleSubmit = (...args: any[]) => {
    if (isLoading) return; // Prevent multiple submissions
    setIsLoading(true);

    if (LicenseDialogType.Unbind === licenseDialogType) {
      handleUnbind();
    }

    if (LicenseDialogType.Activation === licenseDialogType) {
      handleActivation(...args);
    }
  };

  const handleUnbind = () => {
    LicenseService.removeLicense({ license })
      .then((res) => {
        if (res) {
          handleSuccess();
          staticMessage.success(i18n('setting.license.unbindSuccess'));
        } else {
          staticMessage.error(i18n('setting.license.unbindFailed'));
        }
      })
      .catch(() => handleError(i18n('setting.license.unbindFailed')))
      .finally(() => setIsLoading(false));
  };

  const handleActivation = (activeType: 'online' | 'offline') => {
    if (activeType === 'online') {
      LicenseService.validLicense({ license })
        .then((res) => {
          const { needDoubleCheck, codeFromWechat, doubleCheckDisplayEmail } = res || {};

          if (needDoubleCheck) {
            setActivationInfo({ needDoubleCheck, codeFromWechat, doubleCheckDisplayEmail });
            return;
          }

          if (res) {
            handleSuccess();
            staticMessage.success(i18n('setting.license.activationSuccess'));
          } else {
            handleError(i18n('setting.license.activationFailedClickForDetails'));
          }
        })
        .catch(() => handleError(i18n('setting.license.activationFailedClickForDetails')))
        .finally(() => setIsLoading(false));
    }

    if (activeType === 'offline') {
      setIsLoading(true);
      LicenseService.activateCer({ license })
        .then((res) => {
          if (res) {
            handleSuccess();
            staticMessage.success(i18n('setting.license.activationSuccess'));
          } else {
            handleError(i18n('setting.license.activationFailedClickForDetails'));
          }
        })
        .catch(() => handleError(i18n('setting.license.activationFailedClickForDetails')))
        .finally(() => setIsLoading(false));
    }
  };

  const handleDoubleCheckSubmit = (passcode: string) => {
    if (isLoading) return;
    setIsLoading(true);
    LicenseService.checkPasscode({ license, passcode })
      .then((res) => {
        if (res) {
          handleSuccess();
          staticMessage.success(i18n('setting.license.activationSuccess'));
        } else {
          handleError(i18n('setting.license.activationFailedClickForDetails'));
        }
      })
      .catch(handleError)
      .finally(() => setIsLoading(false));
  };

  const handleLicenseChange = (v: string) => {
    setLicense(v.trim());
    setActivationError(null);
  };

  const showErrorDetails = () => {
    openWebPage(`${appUrlConfig.DOCS_URL}/docs/question/fail-to-activate-license`);
  };

  if (LicenseDialogType.Activation === licenseDialogType && activationInfo?.needDoubleCheck) {
    return (
      <DoubleCheckDialog
        open={open}
        doubleCheckDisplayEmail={activationInfo.doubleCheckDisplayEmail}
        license={license}
        onClose={handleClose}
        onSubmit={handleDoubleCheckSubmit}
        isLoading={isLoading}
        {...rest}
      />
    );
  }

  const commonProps = {
    open,
    license,
    activationError,
    onLicenseChange: handleLicenseChange,
    onSubmit: handleSubmit,
    showErrorDetails,
    onClose: handleClose,
    isLoading, // Add this line
    ...rest,
  };

  if (LicenseDialogType.Unbind === licenseDialogType) {
    return <UnbindDialog {...commonProps} />;
  }

  if (LicenseDialogType.Activation === licenseDialogType) {
    return <ActivationDialog {...commonProps} appUrlConfig={appUrlConfig} />;
  }
};

export default LicenseDialog;
