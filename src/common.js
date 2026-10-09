// Helpers shared by the shelf, settings and configure windows.

export const { invoke, convertFileSrc } = window.__TAURI__.core;
const { emit } = window.__TAURI__.event;

export const SOURCE_LABELS = { steam: "Steam", epic: "Epic Games", xbox: "Xbox", custom: "Added by you" };

export function el(tag, className, props = {}) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  return Object.assign(node, props);
}

export function setMessage(node, text, isError = false) {
  node.textContent = text;
  node.classList.toggle("error", isError);
}

/** Tells the shelf window to reload its games. */
export function reloadShelf() {
  emit("reload-shelf");
}

let audio = null;

/** A short wooden "tock" as a box slides out. `volume` is 0-100. */
export function playHoverSound(volume = 30) {
  if (volume <= 0) return;
  audio ??= new AudioContext();
  const now = audio.currentTime;
  const osc = audio.createOscillator();
  const gain = audio.createGain();
  osc.type = "sine";
  osc.frequency.setValueAtTime(900, now);
  osc.frequency.exponentialRampToValueAtTime(550, now + 0.05);
  // 100% is the original loudness.
  gain.gain.setValueAtTime(0.06 * (volume / 100), now);
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.08);
  osc.connect(gain).connect(audio.destination);
  osc.start(now);
  osc.stop(now + 0.09);
}

// No browser menu ("Reload", "Inspect"…) outside text fields.
document.addEventListener("contextmenu", (e) => {
  if (!e.target.closest("input, textarea")) e.preventDefault();
});
