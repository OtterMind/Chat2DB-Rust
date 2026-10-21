import React from 'react';
import * as ReactDom from 'react-dom/client';
import AppTheme from '@/components/AppTheme';

export function insertOpenScreenAnimationExpand(element: React.ReactElement) {
  const open_screen_animation_expand = document.getElementById('open_screen_animation_expand');
  if (open_screen_animation_expand) {
    ReactDom.createRoot(open_screen_animation_expand).render(<AppTheme>{element}</AppTheme>);
  }
}

// Get the element with id open_screen_animation and delete it
export function removeOpenScreenAnimation() {
  const openScreenAnimation = document.getElementById('open_screen_animation');
  if (openScreenAnimation) {
    openScreenAnimation.remove();
  }
}
