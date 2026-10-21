import { LoginDetailType } from '@/typings/enterprise/oauth';
import { PayStatus } from '@/constants/pricing';
import { GuideDialogStatus, LicenseDialogType } from '@/components/GuideDialog/type';
import { HookAPI } from 'antd/es/modal/useModal';
export interface MiscState {
  loginType: LoginDetailType;
  payStatus: PayStatus | null;
  /** Open the boot popup */
  openGuideDialog: boolean;
  guideDialogStatus: GuideDialogStatus;
  /** Trigger fireworks */
  triggerConfetti: boolean;
  /** Open the activation pop-up box */
  openLinenseDialog: boolean;
  /** Activate popup type */
  licenseDialogType: LicenseDialogType | null;
  deleteModal: HookAPI | null;
  /** Workspace empty status AI introduction is closed */
  workspaceAiIntroDismissed: boolean;
}

export const initialMiscState: MiscState = {
  loginType: LoginDetailType.EMAIL_PASSCODE,
  payStatus: null,
  openGuideDialog: false,
  guideDialogStatus: GuideDialogStatus.FirstLogin,
  openLinenseDialog: false,
  licenseDialogType: null,
  triggerConfetti: false,
  deleteModal: null,
  workspaceAiIntroDismissed: false,
};
