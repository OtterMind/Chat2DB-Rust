import React from 'react';
export interface INavItem {
  key: string;
  icon: any;
  component?: React.ReactNode;
  isLoad?: boolean;
  name: string;
  isRoute?: boolean;
  routePath?: string;
  onClick?: () => void;
}
