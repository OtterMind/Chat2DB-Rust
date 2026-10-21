// Model startup task
export interface ILLMStartup {
  /**
   * Model startup task id
   */
  id: number;
  /**
   *Organization id
   */
  organizationId: number;
  /**
   * Model startup task name
   */
  title: string;
  /**
   * Model startup task description
   */
  description: string;
  /**
   * Model startup task status
   */
  status: string;
  /**
   * Model name
   */
  modelName: string;
  /**
   *Model path
   */
  modelPath: string;
  /**
   * Number of graphics cards
   */
  deviceNum: number;
  /**
   *Device type
   */
  deviceType: string;
  /**
   * Quantification type
   */
  quantizationType: string;
  /**
   * Start log countdown to 100 lines
   */
  startupLog: string;
  /**
   * Creation time
   */
  createdTime: number;
}

export type ILLMStartupListItem = Omit<ILLMStartup, 'organizationId' | 'startupLog' | 'status' | 'createdTime'>;
