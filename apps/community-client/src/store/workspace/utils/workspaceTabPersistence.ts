import { IWorkspaceTab } from '@/typings';

export function getPersistableWorkspaceTabList(workspaceTabList?: IWorkspaceTab[] | null) {
  if (!workspaceTabList?.length) {
    return workspaceTabList || null;
  }

  try {
    return JSON.parse(
      JSON.stringify(workspaceTabList, (_key, value) => {
        if (typeof value === 'function') {
          return undefined;
        }
        return value;
      }),
    ) as IWorkspaceTab[];
  } catch {
    return workspaceTabList.map((tab) => ({
      id: tab.id,
      type: tab.type,
      title: tab.title,
      uniqueData: tab.uniqueData
        ? Object.fromEntries(Object.entries(tab.uniqueData).filter(([, value]) => typeof value !== 'function'))
        : undefined,
    }));
  }
}

export function getPersistableActiveConsoleId(params: {
  activeConsoleId?: string | number | null;
  workspaceTabList?: IWorkspaceTab[] | null;
}) {
  const { activeConsoleId, workspaceTabList } = params;
  if (!workspaceTabList?.length) {
    return null;
  }
  if (workspaceTabList.some((tab) => tab.id === activeConsoleId)) {
    return activeConsoleId || null;
  }
  return workspaceTabList[0].id;
}
