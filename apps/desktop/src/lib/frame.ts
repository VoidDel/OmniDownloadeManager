// Animation frames in windows that are not shown yet.

/**
 * Runs `f` on the next animation frame, or after `ms` when no frame comes: WebKitGTK (Linux)
 * pauses animation frames in a window that is not shown, and the main window stays hidden until
 * its first frame is ready. Returns a function that cancels it.
 */
export function nextFrame(f: () => void, ms = 100): () => void {
  let done = false;
  const cancel = () => {
    done = true;
    cancelAnimationFrame(frame);
    clearTimeout(timer);
  };
  const run = () => {
    if (done) return;
    cancel();
    f();
  };
  const frame = requestAnimationFrame(run);
  const timer = setTimeout(run, ms);
  return cancel;
}
