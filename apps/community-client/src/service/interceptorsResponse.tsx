import { GuideDialogStatus } from '@/components/GuideDialog/type';
import { ErrorCode } from '@/constants/request';
import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import i18n from '@/i18n';
import { OrgNavType } from '@/constants/organization';
import { IErrorLevel, PermissionError } from '@/service/base';
import { useGlobalStore } from '@/store/global';
import { useOrgStore } from '@/store/organization';
import { useUserStore } from '@/store/user';
import { isDesktop, isHashHistoryEnv, isOfflineEnv, isProEdition } from '@/utils/env';
import { isEmbedIframePage } from '@/utils/iframe';
import { staticMessage, staticModal } from '@chat2db/ui';
import { history } from 'umi';

interface IProps {
  errorCode: ErrorCode;
  errorMessage: string;
  requestParams: any;
  errorLevel: IErrorLevel;
  permissionError: PermissionError;
}

const interceptorsResponse = ({ errorCode, errorMessage, requestParams, permissionError }: IProps) => {
  if (!errorCode) {
    return;
  }

  const isEmbedIframe = !!useGlobalStore.getState().isEmbedIframe || isEmbedIframePage();

  if (errorCode === ErrorCode.NeedLoggedIn && runtimeEditionConfig.commercialAccount && !isOfflineEnv) {
    const currentPath = window.location.pathname;
    const currentSearch = window.location.search;
    const currentHash = window.location.hash;
    const fullPath = `${currentPath}${currentSearch}${currentHash}`;
    const encodedRedirect = encodeURIComponent(fullPath);
    let loginUrl = `/login?redirect=${encodedRedirect}`;

    if (!isHashHistoryEnv && !isDesktop) {
      const currentOrigin = window.location.origin;
      loginUrl = `${currentOrigin}/login?redirect=${encodedRedirect}`;
    }

    history.push(loginUrl);
    return;
  }

  if (errorCode === ErrorCode.FreeTrialUSageLimit) {
    if (isEmbedIframe) {
      return;
    }

    if (isOfflineEnv) {
      staticMessage.info(i18n('common.text.aiUsageLimit'));

      useGlobalStore.getState().setGuideDialogStatus(GuideDialogStatus.OfflineTrialExpired);
      useGlobalStore.getState().setOpenGuideDialog(true);
      return;
    }

    if (isProEdition) {
      useUserStore.getState().setPricingModalStatus(ErrorCode.FreeTrialUSageLimit);
    }
    return;
  }

  if (errorCode === ErrorCode.NetworkError) {
    staticMessage.error(i18n('common.text.notOnline'));
    return;
  }

  if (errorCode === ErrorCode.LicenseBindCountExceeds) {
    staticMessage.error(i18n('license.deviceLimit'));
    return;
  }

  if (errorCode === ErrorCode.LicenseNotSupported) {
    staticMessage.error(i18n('license.licenseNotSupported'));
    return;
  }

  if ([ErrorCode.OfflineInvalidTrial, ErrorCode.OfflineInvalidDevice].includes(errorCode)) {
    if (isEmbedIframe) {
      return;
    }

    useGlobalStore.getState().setGuideDialogStatus(GuideDialogStatus.OfflineTrial);
    useGlobalStore.getState().setOpenGuideDialog(true);
    return;
  }

  if (ErrorCode.OfflineTrialExpired === errorCode) {
    if (isEmbedIframe) {
      return;
    }

    useGlobalStore.getState().setGuideDialogStatus(GuideDialogStatus.OfflineTrialExpired);
    useGlobalStore.getState().setOpenGuideDialog(true);
    return;
  }

  if (ErrorCode.OfflineLicenseExpired === errorCode) {
    if (isEmbedIframe) {
      return;
    }

    useGlobalStore.getState().setGuideDialogStatus(GuideDialogStatus.OfflineLicenseExpired);
    useGlobalStore.getState().setOpenGuideDialog(true);
    return;
  }

  if (
    [ErrorCode.NoDataAccessPermission, ErrorCode.NoScriptPermission].includes(errorCode) &&
    permissionError === 'apply'
  ) {
    const { sql, dataSourceId, dataSourceName, databaseName, schemaName } = requestParams;

    staticModal.confirm({
      title: i18n('team.permission.modal.title'),
      content: <div style={{ whiteSpace: 'pre-wrap' }}>{errorMessage}</div>,
      okText: i18n('team.permission.modal.OkText'),
      cancelText: i18n('team.permission.modal.cancelText'),
      onOk: () => {
        const props = {
          applyType: errorCode === ErrorCode.NoDataAccessPermission ? 'data' : 'script',
          script: sql,
          dataSourceId,
          databaseName,
          dataSourceName,
          schemaName,
        };
        const { setMainPageActiveTab } = useGlobalStore.getState();
        setMainPageActiveTab({ page: 'team' });
        const { setApplyProps, setOrgNav } = useOrgStore.getState();
        setApplyProps(props);
        setOrgNav(OrgNavType.ApplyList);

        // history.push(`/team`);
      },
    });
  }
};

export default interceptorsResponse;
