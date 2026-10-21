export function getTableKeyboardNavigationIndex(key: string, currentIndex: number, itemCount: number) {
  if (currentIndex < 0 || itemCount <= 0) {
    return null;
  }
  if (key === 'ArrowUp') {
    return Math.max(0, currentIndex - 1);
  }
  if (key === 'ArrowDown') {
    return Math.min(itemCount - 1, currentIndex + 1);
  }
  if (key === 'Home') {
    return 0;
  }
  if (key === 'End') {
    return itemCount - 1;
  }
  return null;
}

export function getDirectoryJumpIndex(
  key: string,
  currentIndex: number,
  directoryFlags: boolean[],
) {
  if (
    currentIndex < 0 ||
    currentIndex >= directoryFlags.length ||
    (key !== 'ArrowUp' && key !== 'ArrowDown')
  ) {
    return null;
  }
  const step = key === 'ArrowUp' ? -1 : 1;
  for (let index = currentIndex + step; index >= 0 && index < directoryFlags.length; index += step) {
    if (directoryFlags[index]) {
      return index;
    }
  }
  return currentIndex;
}

export function shouldActivateTableAction(key: string) {
  return key === 'Enter' || key === ' ';
}

export function shouldToggleTreeRowWithArrow(key: string, expanded: boolean) {
  return (key === 'ArrowLeft' && expanded) || (key === 'ArrowRight' && !expanded);
}
