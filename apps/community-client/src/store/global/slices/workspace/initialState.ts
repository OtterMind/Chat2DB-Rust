export interface WorkspaceState {
  activeConsoles: Array<{
    id: string;
    [key: string]: any;
  }>;
  createConsole?: () => void;
  removeConsole?: (id: string) => void;
}

export const initialWorkspaceState: WorkspaceState = {
  activeConsoles: [],
};
