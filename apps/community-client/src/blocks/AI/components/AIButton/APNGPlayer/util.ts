import parseBuffer, { APNG, Frame } from 'apng-js';

export async function resizeAPNG(buffer, targetWidth, targetHeight) {
  // parses APNG data
  const apng = await parseBuffer(buffer);
  if (apng instanceof Error) {
    console.error('Parse APNG failed:', apng);
    return;
  }

  // creates an off-screen canvas for drawing original frames
  const sourceCanvas = document.createElement('canvas');
  sourceCanvas.width = apng.width;
  sourceCanvas.height = apng.height;
  const sourceCtx = sourceCanvas.getContext('2d');

  // creates a canvas for zooming
  const targetCanvas = document.createElement('canvas');
  targetCanvas.width = targetWidth;
  targetCanvas.height = targetHeight;
  const targetCtx = targetCanvas.getContext('2d');

  // stores scaled frame data
  const resizedFrames = [];

  // processes each frame
  for (const frame of apng.frames) {
    // clear source canvas
    sourceCtx.clearRect(0, 0, sourceCanvas.width, sourceCanvas.height);

    // Draw the current frame to the source canvas
    sourceCtx.drawImage(frame.imageElement, frame.left, frame.top);

    // Clear target canvas
    targetCtx.clearRect(0, 0, targetWidth, targetHeight);

    // Scale and draw the content of the source canvas to the target canvas
    targetCtx.drawImage(sourceCanvas, 0, 0, targetWidth, targetHeight);

    // Gets the scaled image data
    const resizedImageData = targetCtx.getImageData(0, 0, targetWidth, targetHeight);

    // creates a new frame object
    const resizedFrame = {
      imageData: resizedImageData,
      delay: frame.delay,
      disposeOp: frame.disposeOp,
      blendOp: frame.blendOp,
      left: Math.round(frame.left * (targetWidth / apng.width)),
      top: Math.round(frame.top * (targetHeight / apng.height)),
      width: targetWidth,
      height: targetHeight,
    } as Frame;

    resizedFrames.push(resizedFrame);
  }

  // Creates a new APNG object
  const resizedAPNG = new APNG();
  resizedAPNG.width = targetWidth;
  resizedAPNG.height = targetHeight;
  resizedAPNG.frames = resizedFrames;
  resizedAPNG.numPlays = apng.numPlays;
  return resizedAPNG;
}

// usage example
export async function processAPNG(apngBuffer) {
  try {
    const resizedAPNG = await resizeAPNG(apngBuffer, 32, 32);
    // can use resizedAPNG.createObjectURL() to get the blob URL
    // or use resizedAPNG.getBufferAsync() to get the buffer
    return resizedAPNG;
  } catch (error) {
    console.error('处理 APNG 时发生错误:', error);
    throw error;
  }
}
