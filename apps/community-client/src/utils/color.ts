export function hexToRgba(color, alpha) {
  // Check if rgba
  if (color.startsWith('rgba')) {
    return color; // It is already rgba and does not need to be converted.
  }
  // Check if it is rgb
  if (color.startsWith('rgb')) {
    return `${color.replace('rgb', 'rgba').replace(')', `, ${alpha / 100})`)}`;
  }
  // Determine whether it is in hex format
  if (color.startsWith('#')) {
    // Remove # and convert based on length
    let hex = color.slice(1);
    if (hex.length === 3) {
      hex = hex
        .split('')
        .map((c) => c + c)
        .join('');
    }
    // Convert to RGB value
    const r = parseInt(hex.slice(0, 2), 16);
    const g = parseInt(hex.slice(2, 4), 16);
    const b = parseInt(hex.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha / 100})`;
  }
  // If the format is incorrect, return the original color
  return color;
}

// Get the theme mode of the current system
export function getSystemThemeMode() {
  if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}
