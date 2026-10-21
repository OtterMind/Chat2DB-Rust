import React from 'react';

export interface SuggestionItem {
  label: string;
  value: string;
  tableType: string;

  icon?: React.ReactNode;
  children?: SuggestionItem[];
  extra?: React.ReactNode;
}
export type SuggestionItems = SuggestionItem[];
