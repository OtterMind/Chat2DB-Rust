export const removeLeftNav = () => {
  const leftContainer = document.getElementById('left-nav-container');
  // Hide left navigation bar
  if (leftContainer) {
    leftContainer.style.display = 'none';
  }
};
