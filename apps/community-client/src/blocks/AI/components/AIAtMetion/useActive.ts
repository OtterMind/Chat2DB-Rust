import { useEvent } from 'rc-util';
import React, { useState } from 'react';

import { SuggestionItem } from './interface';

export default function useActive(
  items: SuggestionItem[],
  open: boolean,
  onSelect: (value: string[]) => void,
  onCancel: () => void,
) {
  const [activePaths, setActivePaths] = useState<string[]>([]);

  /** Get items by column index */
  const getItems = (colIndex: number, paths = activePaths) => {
    let currentItems = items;

    for (let i = 0; i < colIndex - 1; i += 1) {
      const activePath = paths[i];
      const activeItem = currentItems.find((item) => item.value === activePath);

      if (!activeItem) {
        break;
      }

      currentItems = activeItem.children || [];
    }

    return currentItems;
  };

  const getValues = (paths: string[]) => {
    return paths.map((path, index) => {
      const currentItems = getItems(index + 1, paths);
      const currentItem = currentItems.find((item) => item.value === path);

      return currentItem?.value;
    }) as string[];
  };

  const offsetRow = (offset: number) => {
    const currentColIndex = activePaths.length || 1;

    const currentItems = getItems(currentColIndex);
    const currentRowIndex = currentItems.findIndex((item) => item.value === activePaths[currentColIndex - 1]);
    const itemCount = currentItems.length;

    const nextItem = currentItems[(currentRowIndex + offset + itemCount) % itemCount];
    setActivePaths([...activePaths.slice(0, currentColIndex - 1), nextItem.value]);

    // Add a delay to wait for the DOM to update before scrolling
    setTimeout(() => {
      // Gets the currently selected option element
      const activeElement = document.querySelector('.ant-cascader-menu-item-active');
      if (activeElement) {
        // ensures that the element is scrolled into the visible area
        activeElement.scrollIntoView({
          block: 'center',
          behavior: 'smooth',
        });
      }
    }, 0);
  };

  const offsetNext = () => {
    const currentColIndex = activePaths.length;
    const nextItems = getItems(currentColIndex + 1);

    // If there are sub-items, select the first sub-item
    if (nextItems.length > 0) {
      setActivePaths([...activePaths, nextItems[0].value]);

      // Wait for the DOM to update before scrolling to the newly selected item
      setTimeout(() => {
        const activeElement = document.querySelector('.ant-cascader-menu-item-active');
        if (activeElement) {
          activeElement.scrollIntoView({
            block: 'start',
            behavior: 'smooth',
          });
        }
      }, 0);
    }
  };

  const offsetPrev = () => {
    // If there is a previous level, return to the previous level
    if (activePaths.length > 1) {
      setActivePaths(activePaths.slice(0, -1));

      // Wait for the DOM to update before scrolling to the newly selected item
      setTimeout(() => {
        const activeElement = document.querySelector('.ant-cascader-menu-item-active');
        if (activeElement) {
          activeElement.scrollIntoView({
            block: 'nearest',
            behavior: 'smooth',
          });
        }
      }, 0);
    }
  };

  const onKeyDown = useEvent((e: React.KeyboardEvent) => {
    if (!open) {
      return;
    }
    switch (e.key) {
      case 'ArrowDown': {
        offsetRow(1);
        e.preventDefault();
        break;
      }

      case 'ArrowUp': {
        offsetRow(-1);
        e.preventDefault();
        break;
      }

      case 'ArrowRight': {
        offsetNext();
        e.preventDefault();
        break;
      }

      case 'ArrowLeft': {
        offsetPrev();
        e.preventDefault();
        break;
      }

      case 'Enter': {
        // Submit if not have children
        if (getItems(activePaths.length + 1).length === 0) {
          onSelect(getValues(activePaths));
        }
        e.preventDefault();
        break;
      }

      case 'Escape': {
        onCancel();
        e.preventDefault();
        break;
      }
      default: {
        break;
      }
    }
  });

  React.useEffect(() => {
    if (open && items?.[0]?.value) {
      setActivePaths([items[0].value]);
    }
  }, [open, items]);

  return [activePaths, onKeyDown] as const;
}
