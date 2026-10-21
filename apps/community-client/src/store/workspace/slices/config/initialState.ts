export interface ConfigState {
  layout: {
    panelLeft: boolean;
    panelLeftWidth: number;
    panelRight: boolean;
    panelRightWidth: number;
  };
}

export const initConfigState: ConfigState = {
  layout: {
    panelLeft: true,
    panelRight: true,
    panelLeftWidth: 260,
    panelRightWidth: 300,
  },
};
