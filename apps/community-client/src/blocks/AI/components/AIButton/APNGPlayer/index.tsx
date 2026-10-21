import { useEffect, useRef } from 'react';
import parseAPNG, { APNG } from 'apng-js';
import apngUrl from '@/assets/img/ai/ai-click.png';
import Player from 'apng-js/types/library/player';

interface APNGPlayerProps {
  onClick: () => void;
}

const APNGPlayer = (props: APNGPlayerProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const APNGRef = useRef<APNG>(null);
  const playerRef = useRef<Player | null>(null);

  useEffect(() => {
    loadAPNG();
  }, []);

  const loadAPNG = async () => {
    const response = await fetch(apngUrl);
    const buffer = await response.arrayBuffer();
    const apng = parseAPNG(buffer);
    // return apng;
    if (apng instanceof Error) {
      console.error('Parse APNG failed:', apng);
      return;
    }

    // New code: Set the width and height of APNG to 32x32
    apng.width = 32;
    apng.height = 32;
    console.log('apng', apng);

    APNGRef.current = apng;

    const canvas = canvasRef.current;
    if (!canvas) return;

    canvas.width = apng.width; // apng.width;
    canvas.height = apng.height; // apng.height;
    // canvas.width = 32;
    // canvas.height = 32;
    const framesLength = apng.frames.length;

    console.log('framesLength', framesLength);

    await apng.createImages();

    console.log('apng.frames', apng.frames);

    // New code: adjust the size of each frame

    const player = await apng.getPlayer(canvas.getContext('2d') as CanvasRenderingContext2D);
    player.playbackRate = 1.5;
    playerRef.current = player;
    const em = playerRef.current.emit;
    playerRef.current.emit = (event, ...args) => {
      console.log('event', event, args);
      em(event, ...args);
      if (event === 'frame' && args[0] === framesLength - 1) {
        // console.log('frame', args[0]);
        playerRef.current?.pause();
        APNGRef.current?.frames.reverse();
      }
    };
  };

  const playAnimation = async () => {
    playerRef.current?.stop();
    playerRef.current?.play();
    props.onClick();
  };

  // return <canvas ref={canvasRef} onClick={playAnimation} style={{ cursor: 'pointer' }} />;
  return <canvas ref={canvasRef} onClick={playAnimation} style={{ cursor: 'pointer' }} />;
};

export default APNGPlayer;
