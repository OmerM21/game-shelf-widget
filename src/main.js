import { invoke, convertFileSrc, el, setMessage, playHoverSound } from "./common.js";

const { listen } = window.__TAURI__.event;

const shelf = document.getElementById("shelf");
const status = document.getElementById("status");

/** How many games fetch artwork at the same time. */
const ARTWORK_CONCURRENCY = 4;

/** Bumped on every shelf reload so stale artwork results are dropped. */
let loadGeneration = 0;

const setStatus = (text, isError) => setMessage(status, text, isError);

// ---------------------------------------------------------------- Shelf

/** A stable dark color per game, shown until (or instead of) artwork. */
function fallbackColor(name) {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)) | 0;
  return `hsl(${Math.abs(hash) % 360} 35% 28%)`;
}

function createBox(game) {
  const box = el("button", "box");
  box.dataset.name = game.name;
  box.style.setProperty("--fallback", fallbackColor(game.name));
  box.setAttribute("aria-label", `Play ${game.name}`);

  const spine = el("div", "spine");
  spine.append(el("span", "spine-title", { textContent: game.name }));

  const cover = el("div", "cover");
  cover.append(el("span", "cover-title", { textContent: game.name }));

  const face = el("div", "face");
  face.append(spine, cover);
  box.append(face);
  box.addEventListener("click", () => launch(game));
  box.addEventListener("mouseenter", () => focusBox(box, { withSound: true }));
  box.addEventListener("focus", () => focusBox(box));
  box.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    focusBox(box);
    openGameMenu(e, game, box);
  });
  return box;
}

function applyArtwork(box, art) {
  const spine = box.querySelector(".spine");
  const cover = box.querySelector(".cover");

  // The spine is a crop of the wide hero banner, or the cover if there's none.
  const spineArt = art.hero ?? art.cover;
  if (spineArt) {
    const background = el("div", "spine-art");
    background.style.backgroundImage = `url("${convertFileSrc(spineArt)}")`;
    spine.prepend(background);
  }

  if (art.logo) {
    const title = spine.querySelector(".spine-title");
    const logo = el("img", "spine-logo", { src: convertFileSrc(art.logo), alt: "" });
    // Keep the text title if the logo fails to load.
    logo.addEventListener("load", () => title.remove());
    logo.addEventListener("error", () => logo.remove());
    spine.append(logo);
  }

  if (art.cover) {
    // Covers load the first time their game is pulled out: only one shows at a
    // time, and every loaded image stays decoded in memory.
    box.dataset.cover = convertFileSrc(art.cover);
    if (box.classList.contains("active")) loadCover(box);
  }
}

function loadCover(box) {
  const src = box.dataset.cover;
  if (!src || box.dataset.coverLoaded) return;
  box.dataset.coverLoaded = "1";
  const cover = box.querySelector(".cover");
  const img = el("img", "", { src, alt: "" });
  img.addEventListener("load", () => cover.querySelector(".cover-title")?.remove());
  cover.prepend(img);
}

/**
 * Pulls `active` out to face the viewer and tilts the others away from it.
 * One box is always active so the shelf's total width never changes; if it
 * did, re-centering would slide a different box under the cursor.
 */
function focusBox(active, { withSound = false } = {}) {
  if (withSound && preferences.hoverSound && !active.classList.contains("active")) {
    playHoverSound(preferences.hoverVolume);
  }
  const boxes = [...shelf.children];
  const index = boxes.indexOf(active);
  boxes.forEach((box, i) => {
    box.classList.toggle("active", i === index);
    box.classList.toggle("right", i > index);
  });
  loadCover(active);
}

/** Saved user preferences, refreshed whenever the shelf loads. */
let preferences = { sort: "name", hoverSound: false };


async function hideGame(game, box) {
  try {
    await invoke("set_game_hidden", { id: game.id, hidden: true });
  } catch (err) {
    setStatus(String(err), true);
    return;
  }

  // Keep a game pulled out: the next one, or the previous if this was last.
  const neighbor = box.nextElementSibling ?? box.previousElementSibling;
  const wasActive = box.classList.contains("active");
  box.remove();
  if (!neighbor) {
    setStatus("All games are hidden. Unhide them in settings.");
  } else if (wasActive) {
    focusBox(neighbor);
  }
}

// ---------------------------------------------------------------- Game menu

const gameMenu = document.getElementById("game-menu");
let menuTarget = null;

function openGameMenu(event, game, box) {
  menuTarget = { game, box };
  gameMenu.hidden = false;

  // Opened from the keyboard (Menu key / Shift+F10) there's no pointer position.
  const fromKeyboard = event.clientX === 0 && event.clientY === 0;
  const rect = box.getBoundingClientRect();
  const x = fromKeyboard ? rect.left + rect.width / 2 : event.clientX;
  const y = fromKeyboard ? rect.top + rect.height / 2 : event.clientY;

  // Keep the menu inside the window.
  const { offsetWidth: w, offsetHeight: h } = gameMenu;
  gameMenu.style.left = `${Math.min(x, window.innerWidth - w - 8)}px`;
  gameMenu.style.top = `${Math.min(y, window.innerHeight - h - 8)}px`;
  gameMenu.querySelector("button").focus();
}

function closeGameMenu() {
  if (gameMenu.hidden) return;
  gameMenu.hidden = true;
  menuTarget?.box.focus({ preventScroll: true });
  menuTarget = null;
}

gameMenu.addEventListener("click", (e) => {
  const action = e.target.closest("button")?.dataset.action;
  const target = menuTarget;
  closeGameMenu();
  if (!target) return;
  if (action === "launch") launch(target.game);
  if (action === "configure") invoke("open_configure", { id: target.game.id });
  if (action === "hide") hideGame(target.game, target.box);
});

gameMenu.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...gameMenu.querySelectorAll("button")];
  const step = e.key === "ArrowDown" ? 1 : -1;
  const next = (items.indexOf(document.activeElement) + step + items.length) % items.length;
  items[next].focus();
});

document.addEventListener("pointerdown", (e) => {
  if (!gameMenu.contains(e.target)) closeGameMenu();
});
window.addEventListener("blur", closeGameMenu);
window.addEventListener("resize", closeGameMenu);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeGameMenu();
});

async function launch(game) {
  setStatus("");
  try {
    await invoke("launch_game", { id: game.id });
  } catch (err) {
    setStatus(String(err), true);
  }
}

async function loadArtwork(entries, generation) {
  const queue = [...entries];
  let firstError = null;

  async function worker() {
    while (queue.length > 0 && generation === loadGeneration) {
      const { game, box } = queue.shift();
      try {
        const art = await invoke("get_artwork", { id: game.id });
        if (generation === loadGeneration) applyArtwork(box, art);
      } catch (err) {
        firstError ??= String(err);
      }
    }
  }

  await Promise.all(Array.from({ length: ARTWORK_CONCURRENCY }, worker));
  if (firstError && generation === loadGeneration) setStatus(firstError, true);
}

async function loadShelf() {
  const generation = ++loadGeneration;
  shelf.replaceChildren();
  setStatus("");

  let listing;
  try {
    listing = await invoke("list_games");
  } catch (err) {
    setStatus(String(err), true);
    return;
  }
  const { games, hidden, warning } = listing;
  preferences = listing.preferences;
  applySlant(preferences.slant);

  if (games.length === 0) {
    const empty = hidden.length > 0
      ? "All games are hidden. Unhide them in settings."
      : "No games yet. Add one in settings.";
    setStatus(warning ?? empty, Boolean(warning));
    return;
  }

  const entries = games.map((game) => ({ game, box: createBox(game) }));
  shelf.append(...entries.map((e) => e.box));
  focusBox(entries[0].box);
  if (warning) setStatus(warning, true);
  await loadArtwork(entries, generation);
}

// ---------------------------------------------------------------- Other windows

document.getElementById("open-settings").addEventListener("click", () => invoke("open_settings"));

// ---------------------------------------------------------------- Resizing

// Dragging the resize button resizes the window from its top-right corner,
// like dragging that corner of a framed window.
document.getElementById("resize-handle").addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  window.__TAURI__.window.getCurrentWindow().startResizeDragging("NorthEast");
});

// ---------------------------------------------------------------- Updates

// Sent by the settings and configure windows, and the tray's "Refresh games".
listen("reload-shelf", () => loadShelf());

/** How much shorter a spine's far edge is, in percent of its height. */
function applySlant(percent) {
  document.documentElement.style.setProperty("--slant", `${percent}%`);
}

// Sent while dragging the slant slider in settings, before it's saved.
listen("slant-preview", (event) => applySlant(event.payload));

// Sent when the hover volume changes in settings, so the shelf needn't reload.
listen("hover-volume", (event) => {
  preferences.hoverVolume = event.payload;
});

loadShelf();
