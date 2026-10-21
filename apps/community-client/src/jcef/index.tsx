import createJcefApi from './base';
import { FileConstants } from '@/constants/file';
import { IUpdateDetail } from '@/typings/settings';
import { LangType } from '@/constants/settings';
import { ThemeAppearance } from '@chat2db/ui';

const jcefApi = {
  // Open web page
  openWebPage: (url: string) => {
    return createJcefApi('open-web-page', { url });
  },
  // Frontend ready
  handleJavaMessageIsReady: () => {
    return createJcefApi('handle-java-message-is-ready');
  },
  // Open file in finder
  revealInExplorer: (path: string) => {
    return createJcefApi('reveal-in-explorer', { path });
  },
  // Get file URL
  selectDirectory: () => {
    return createJcefApi('select-directory');
  },
  // Select SQL file directory
  selectSqlDirectory: () => {
    return createJcefApi('select-sql-directory');
  },
  // Open the recorded SQL file directory
  openSqlDirectory: (params: { path: string }) => {
    return createJcefApi('open-sql-directory', params);
  },
  // Get SQL file directory subnodes
  getSqlDirectoryChildren: (params: { rootToken: string; relativePath: string }) => {
    return createJcefApi('get-sql-directory-children', params);
  },
  // Create a new SQL file directory subnode
  createSqlDirectoryChild: (params: {
    rootToken: string;
    parentRelativePath: string;
    name: string;
    type: 'file' | 'directory';
  }) => {
    return createJcefApi('create-sql-directory-child', params);
  },
  // Save the SQL file to the opened SQL directory
  saveSqlDirectoryFile: (params: { rootToken: string; parentRelativePath: string; name: string; content: string }) => {
    return createJcefApi('save-sql-directory-file', params);
  },
  // Rename SQL file directory subnode
  renameSqlDirectoryChild: (params: { rootToken: string; relativePath: string; name: string }) => {
    return createJcefApi('rename-sql-directory-child', params);
  },
  // Delete SQL file directory subnodes
  deleteSqlDirectoryChild: (params: { rootToken: string; relativePath: string }) => {
    return createJcefApi('delete-sql-directory-child', params);
  },
  // Open the SQL file directory in the terminal
  openSqlDirectoryTerminal: (params: { rootToken: string; relativePath: string }) => {
    return createJcefApi('open-sql-directory-terminal', params);
  },
  // Select file
  selectFile: (params: { fileTypeList: string[]; fileSize?: number; multiple?: boolean }) => {
    return createJcefApi('select-file', params);
  },
  // maximize
  maximizeWindow: () => {
    return createJcefApi('maximize-window');
  },
  // minimize
  minimizeWindow: () => {
    return createJcefApi('minimize-window');
  },
  // Double-click the AppBar
  handleDoubleClickAppBar: () => {
    return createJcefApi('double-click-app-bar');
  },
  // close window
  closeWindow: () => {
    return createJcefApi('close-window');
  },
  // Is it maximizing
  isWindowMaximized: () => {
    return createJcefApi('is-window-maximized');
  },
  // Check for updates
  appCheckUpdate: () => {
    return createJcefApi<IUpdateDetail>('app-check-update');
  },
  // Start downloading hot updates
  triggerDownload: () => {
    return createJcefApi('trigger-download');
  },
  // Start hot update installation
  triggerInstallation: () => {
    return createJcefApi('trigger-installation');
  },
  // Restart app
  restartApp: () => {
    return createJcefApi('restart-app');
  },
  // Set zoom
  webFrameSetZoom: (data: { action: 'zoomIn' | 'zoomOut' | 'zoomReset' }) => {
    return createJcefApi('web-frame-set-zoom', data);
  },
  // Open log
  openLog: () => {
    return createJcefApi('open-log');
  },
  // Open developer tools
  openDevTools: () => {
    return createJcefApi('open-dev-tools');
  },
  // Get mac address
  getMacAddress: () => {
    return createJcefApi('get-mac-address');
  },
  // save file
  saveFile: (data: { fileName: string; fileContent: string; fileType: string }) => {
    return createJcefApi<{ path: string; size: number } | null>('save-file', data);
  },
  // Change file content
  updateFileContent: (data: { filePath: string; fileContent: string }) => {
    return createJcefApi('update-file-content', data);
  },
  // Open local file
  readFile: (path: string) => {
    return createJcefApi<FileConstants>('read-file', { path });
  },
  // The front-end setting information is synchronized with the back-end
  updateSettings: (data: { appearance: ThemeAppearance; language: LangType; enableMcp?: boolean }) => {
    return createJcefApi('update-settings', data);
  },
  // Get clipboard information
  readClipboard: () => {
    return createJcefApi<string>('read-clipboard');
  },
  // Get MCP token
  getMcpToken: () => {
    return createJcefApi<string>('get-mcp-token');
  },
  // Reset MCP token
  resetMcpToken: () => {
    return createJcefApi<string>('reset-mcp-token');
  },
};

export default jcefApi;
