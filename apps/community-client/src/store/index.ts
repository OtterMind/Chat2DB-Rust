import { clearChatStore } from './chat/store';
import { clearDashboardStore } from './dashboard/store';
import { clearCommonStore } from './common/index';
import { clearConnectionStore } from './connection/index';
import { clearGlobalStore } from './global/store';
import { clearOrgStore } from './organization/index';
import { clearUserStore } from './user/index';
import { clearWorkspaceStore } from './workspace/index';
import { clearTreeStore } from './tree/index';

// Clean Store
export const clearStore = () => {
  clearChatStore();
  clearDashboardStore();
  clearCommonStore();
  clearConnectionStore();
  clearGlobalStore();
  clearOrgStore();
  clearUserStore();
  clearWorkspaceStore();
  clearTreeStore();
};
