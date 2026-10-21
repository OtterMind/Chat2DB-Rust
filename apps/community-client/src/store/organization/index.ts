import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import { OrgNavType } from '@/constants/organization';
import organizationService from '@/service/enterprise/organization';
import { IPageParams } from '@/typings';
import {
  IOrganizationVO,
  IUpdateOrganizationVORequest,
  OrganizationType,
  OrgUserRoleCode,
} from '@/typings/enterprise/organization';
import { Subscription } from '@/typings/enterprise/user';
import { refreshPage } from '@/utils';
import { isDesktop, isOfflineEnv } from '@/utils/env';
import { devtools, persist, PersistOptions } from 'zustand/middleware';
import { shallow } from 'zustand/shallow';
import { createWithEqualityFn } from 'zustand/traditional';
import { StateCreator } from 'zustand/vanilla';
import { useGlobalStore } from '../global';
import { useUserStore } from '../user';

interface OrgState {
  /** Global current organization */
  curOrg: IOrganizationVO | null;
  orgList: IOrganizationVO[] | null;
  curOrgSubscription: Subscription | null;
  openCreateOrJoinOrgDialog: boolean;
  isPersonal: boolean;
  isOwner: boolean;
  isAdmin: boolean;
  applyProps: any | null;
  orgNav: OrgNavType;
}

const initialState: OrgState = {
  curOrg: null,
  orgList: null,
  curOrgSubscription: null,
  openCreateOrJoinOrgDialog: false,
  isPersonal: false,
  isOwner: false,
  isAdmin: false,
  applyProps: null,
  orgNav: OrgNavType.TeamSettings,
};

export interface OrgAction {
  /** Whether you are currently an individual */
  curIsPersonalOrg: () => boolean;
  /** Query the list of all organizations */
  queryOrgList: (params?: Partial<IPageParams> | { needCreateOrg: boolean }) => void;
  /** Set organization pop-up box */
  setOpenCreateOrJoinOrgDialog: (open: boolean) => void;
  /** Set the current organization */
  setCurOrg: (curOrg?: IOrganizationVO) => void;
  /** Create new organization */
  createNewOrg: (orgParams: IOrganizationVO) => Promise<IOrganizationVO>;
  /** Get the organization's subscription information */
  querySubscriptionList: () => void;
  /** Update organization */
  updateOrg: (orgParams: IUpdateOrganizationVORequest) => void;
  setApplyProps: (props: any) => void;
  setOrgNav: (nav: OrgNavType) => void;
  clearOrgStore: () => void;
}

export type OrgStore = OrgState & OrgAction;

const buildOrgFlags = (nextOrg?: IOrganizationVO) => ({
  isOwner: runtimeEditionConfig.usesFixedIdentity || !!nextOrg?.roleCodes?.includes(OrgUserRoleCode.SUPER_ADMIN),
  isAdmin:
    runtimeEditionConfig.usesFixedIdentity ||
    !!nextOrg?.roleCodes?.includes(OrgUserRoleCode.ADMIN) ||
    !!nextOrg?.roleCodes?.includes(OrgUserRoleCode.SUPER_ADMIN) ||
    isOfflineEnv,
  isPersonal: nextOrg?.type === OrganizationType.PERSONAL,
});

export const createOrgAction: StateCreator<OrgStore, [['zustand/devtools', never]], [], OrgAction> = (set, get) => ({
  curIsPersonalOrg: () => {
    if (get().curOrg?.type === OrganizationType.PERSONAL) {
      return true;
    }
    return false;
  },
  setOpenCreateOrJoinOrgDialog: (open: boolean) => {
    set({
      openCreateOrJoinOrgDialog: open,
    });
  },
  setCurOrg: async (nextOrg) => {
    const { curOrg } = get();

    set({
      ...buildOrgFlags(nextOrg),
    });

    if (runtimeEditionConfig.usesFixedIdentity) {
      set({
        curOrg: runtimeEditionConfig.fixedOrganization || nextOrg,
      });
      return;
    }

    if (curOrg && nextOrg && curOrg.id !== nextOrg?.id) {
      await organizationService.switchOrg({ id: nextOrg?.id });
    }

    // Switching organizations requires refreshing the page.
    if (curOrg && curOrg.id !== nextOrg?.id) {
      let activeTab = '';
      if (nextOrg?.type === OrganizationType.PERSONAL) {
        useGlobalStore.getState().setMainPageActiveTab({ page: 'workspace' });
        activeTab = 'workspace';
      }
      if (nextOrg?.type === OrganizationType.TEAM) {
        useGlobalStore.getState().setMainPageActiveTab({ page: 'team' });
        activeTab = 'team';
      }
      if (activeTab) {
        if (!isDesktop) {
          const href = window.location.origin + '/' + activeTab;
          window.history.pushState({}, '', href);
        } else {
          // window.location.hash = `#/${activeTab}`;
        }
        setTimeout(() => {
          refreshPage();
        }, 200);
      }
    }

    set({
      curOrg: nextOrg,
    });
  },
  queryOrgList: async (params) => {
    if (runtimeEditionConfig.usesFixedIdentity && runtimeEditionConfig.fixedOrganization) {
      set({
        orgList: [runtimeEditionConfig.fixedOrganization],
        curOrg: runtimeEditionConfig.fixedOrganization,
        ...buildOrgFlags(runtimeEditionConfig.fixedOrganization),
      });
      return;
    }
    const res = await organizationService.getOrganizationList(params || {});
    set({ orgList: res });
  },
  createNewOrg: async (orgParams) => {
    if (runtimeEditionConfig.usesFixedIdentity && runtimeEditionConfig.fixedOrganization) {
      return Promise.resolve(runtimeEditionConfig.fixedOrganization);
    }
    const org = await organizationService.createOrganization(orgParams);
    get().queryOrgList();
    return Promise.resolve(org);
  },
  querySubscriptionList: () => {
    if (!runtimeEditionConfig.remoteSubscription) {
      return;
    }
    organizationService.getSubscriptionList().then((res) => {
      set({ curOrgSubscription: res?.[0] || null });
    });
  },
  updateOrg: (orgParams: IUpdateOrganizationVORequest) => {
    if (runtimeEditionConfig.usesFixedIdentity) {
      return;
    }

    organizationService.updateOrganization(orgParams).then(() => {
      get().queryOrgList();
      const { queryCurUser } = useUserStore.getState();
      queryCurUser();
    });
  },
  setApplyProps: (props: any) => {
    set({
      applyProps: props,
    });
  },
  setOrgNav: (nav: OrgNavType) => {
    set({
      orgNav: nav,
    });
  },
  clearOrgStore: () => {
    set(initialState);
  },
});

const createStore: StateCreator<OrgStore, [['zustand/devtools', never]]> = (...parameters) => ({
  ...initialState,
  ...createOrgAction(...parameters),
});

type GlobalPersist = Pick<OrgStore, 'curOrg'>;

// local-storage Options
const persistOptions: PersistOptions<OrgStore, GlobalPersist> = {
  name: runtimeEditionConfig.orgStoreName,
  partialize: (state) => ({
    curOrg: state.curOrg,
    orgList: state.orgList,
  }),
};

export const useOrgStore = createWithEqualityFn<OrgStore>()(
  persist(
    devtools(createStore, {
      name: runtimeEditionConfig.orgStoreName,
    }),
    persistOptions,
  ),
  shallow,
);

// // Clean up the store
export const clearOrgStore = () => {
  useOrgStore.setState(initialState);
};
