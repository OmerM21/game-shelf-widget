import { invoke, el, setMessage, reloadShelf, playHoverSound, SOURCE_LABELS } from "./common.js";

const { open: openFileDialog } = window.__TAURI__.dialog;
const { openUrl } = window.__TAURI__.opener;
const currentWindow = window.__TAURI__.window.getCurrentWindow();

const keyInput = document.getElementById("api-key");
const keyMessage = document.getElementById("key-message");
const toggleKey = document.getElementById("toggle-key");
const customList = document.getElementById("custom-games");
const hiddenList = document.getElementById("hidden-games");
const addPath = document.getElementById("add-path");
const addName = document.getElementById("add-name");
const addArgs = document.getElementById("add-args");
const addMessage = document.getElementById("add-message");
const autostartInput = document.getElementById("autostart");
const hoverSoundInput = document.getElementById("hover-sound");
const sortInput = document.getElementById("sort-order");
const generalMessage = document.getElementById("general-message");
const slantInput = document.getElementById("slant");
const slantValue = document.getElementById("slant-value");
const volumeInput = document.getElementById("hover-volume");
const volumeValue = document.getElementById("hover-volume-value");
const { emit } = window.__TAURI__.event;

let preferences = { sort: "name", hoverSound: false };

function renderCustomGames(games, error) {
  if (error) {
    const item = el("li", "empty error", { textContent: `${error}. Fix or delete the file to add games.` });
    customList.replaceChildren(item);
    return;
  }
  if (games.length === 0) {
    customList.replaceChildren(el("li", "empty", { textContent: "You haven't added any games." }));
    return;
  }
  customList.replaceChildren(
    ...games.map((game) => {
      const info = el("div", "game-info");
      info.append(
        el("div", "game-name", { textContent: game.name }),
        el("div", "game-path", { textContent: game.path, title: game.path }),
      );
      const remove = el("button", "remove", { textContent: "Remove", type: "button" });
      remove.setAttribute("aria-label", `Remove ${game.name}`);
      remove.addEventListener("click", async () => {
        try {
          await invoke("remove_custom_game", { id: game.id });
        } catch (err) {
          customList.prepend(el("li", "empty error", { textContent: String(err) }));
          return;
        }
        reloadShelf();
        refresh();
      });
      const item = el("li");
      item.append(info, remove);
      return item;
    }),
  );
}

function renderHiddenGames(games) {
  if (games.length === 0) {
    hiddenList.replaceChildren(el("li", "empty", { textContent: "No hidden games." }));
    return;
  }
  hiddenList.replaceChildren(
    ...games.map((game) => {
      const info = el("div", "game-info");
      info.append(
        el("div", "game-name", { textContent: game.name }),
        el("div", "game-path", { textContent: SOURCE_LABELS[game.source] ?? game.source }),
      );
      const unhide = el("button", "unhide", { textContent: "Unhide", type: "button" });
      unhide.setAttribute("aria-label", `Unhide ${game.name}`);
      unhide.addEventListener("click", async () => {
        try {
          await invoke("set_game_hidden", { id: game.id, hidden: false });
        } catch (err) {
          hiddenList.prepend(el("li", "empty error", { textContent: String(err) }));
          return;
        }
        reloadShelf();
        refresh();
      });
      const item = el("li");
      item.append(info, unhide);
      return item;
    }),
  );
}

async function refresh() {
  const s = await invoke("get_settings");
  preferences = s.preferences;
  if (document.activeElement !== keyInput) keyInput.value = s.apiKey;
  autostartInput.checked = s.autostart;
  if (document.activeElement !== slantInput) showPreferences();
  renderCustomGames(s.customGames, s.configError);
  renderHiddenGames(s.hiddenGames);
  document.getElementById("data-dir").textContent = `Settings and artwork are saved in ${s.dataDir}`;
}

// ---------------------------------------------------------------- General

autostartInput.addEventListener("change", async () => {
  setMessage(generalMessage, "");
  try {
    await invoke("set_autostart", { enabled: autostartInput.checked });
  } catch (err) {
    autostartInput.checked = !autostartInput.checked;
    setMessage(generalMessage, String(err), true);
  }
});

async function savePreferences(changes, { reload = true } = {}) {
  const previous = preferences;
  preferences = { ...preferences, ...changes };
  setMessage(generalMessage, "");
  try {
    await invoke("set_preferences", { preferences });
    if (reload) reloadShelf();
  } catch (err) {
    preferences = previous;
    showPreferences();
    emit("slant-preview", preferences.slant);
    setMessage(generalMessage, String(err), true);
  }
}

function showPreferences() {
  hoverSoundInput.checked = preferences.hoverSound;
  sortInput.value = preferences.sort;
  slantInput.value = preferences.slant;
  slantValue.textContent = describeSlant(preferences.slant);
  volumeInput.value = preferences.hoverVolume;
  volumeValue.textContent = `${preferences.hoverVolume}%`;
  volumeInput.disabled = !preferences.hoverSound;
}

function describeSlant(value) {
  return Number(value) === 0 ? "Flat" : String(value);
}

// Volume: plays a preview at the new level on release.
volumeInput.addEventListener("input", () => {
  volumeValue.textContent = `${volumeInput.value}%`;
});
volumeInput.addEventListener("change", async () => {
  const hoverVolume = Number(volumeInput.value);
  await savePreferences({ hoverVolume }, { reload: false });
  emit("hover-volume", preferences.hoverVolume);
  playHoverSound(preferences.hoverVolume);
});

// Dragging previews the slant live on the shelf; letting go saves it.
slantInput.addEventListener("input", () => {
  slantValue.textContent = describeSlant(slantInput.value);
  emit("slant-preview", Number(slantInput.value));
});
slantInput.addEventListener("change", () => {
  savePreferences({ slant: Number(slantInput.value) }, { reload: false });
});

hoverSoundInput.addEventListener("change", async () => {
  await savePreferences({ hoverSound: hoverSoundInput.checked });
  volumeInput.disabled = !preferences.hoverSound;
  if (preferences.hoverSound) playHoverSound(preferences.hoverVolume); // a preview
});

sortInput.addEventListener("change", () => savePreferences({ sort: sortInput.value }));

// ---------------------------------------------------------------- Rescan

const rescanButton = document.getElementById("rescan");
const rescanMessage = document.getElementById("rescan-message");

/** "15 Steam, 2 Epic, 1 Xbox, 2 added by you" */
function describeCounts(games) {
  const counts = {};
  for (const game of games) counts[game.source] = (counts[game.source] ?? 0) + 1;
  const parts = ["steam", "epic", "xbox", "custom"]
    .filter((source) => counts[source])
    .map((source) => `${counts[source]} ${source === "custom" ? "added by you" : SOURCE_LABELS[source]}`);
  return parts.join(", ") || "none";
}

// Games are read from Steam, Epic and the Xbox app every time the shelf
// loads, so rescanning means listing them again and reloading the shelf.
rescanButton.addEventListener("click", async () => {
  rescanButton.disabled = true;
  setMessage(rescanMessage, "Scanning…");
  try {
    const { games, hidden } = await invoke("list_games");
    const all = [...games, ...hidden];
    setMessage(rescanMessage, `Found ${all.length} games: ${describeCounts(all)}.`);
    reloadShelf();
    refresh();
  } catch (err) {
    setMessage(rescanMessage, String(err), true);
  } finally {
    rescanButton.disabled = false;
  }
});

// ---------------------------------------------------------------- API key

document.getElementById("get-key").addEventListener("click", (e) => {
  e.preventDefault();
  openUrl(e.currentTarget.href);
});

toggleKey.addEventListener("click", () => {
  const hidden = keyInput.type === "password";
  keyInput.type = hidden ? "text" : "password";
  toggleKey.textContent = hidden ? "Hide" : "Show";
});

keyInput.addEventListener("input", () => setMessage(keyMessage, ""));

document.getElementById("key-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  try {
    await invoke("save_api_key", { key: keyInput.value });
    setMessage(keyMessage, keyInput.value.trim() ? "Key saved" : "Key removed");
    reloadShelf();
  } catch (err) {
    setMessage(keyMessage, String(err), true);
  }
});

// ---------------------------------------------------------------- Add a game

document.getElementById("browse").addEventListener("click", async () => {
  const path = await openFileDialog({
    title: "Choose a game",
    multiple: false,
    directory: false,
    filters: [
      { name: "Games and shortcuts", extensions: ["exe", "lnk", "url", "bat", "cmd"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  if (!path) return;

  addPath.value = path;
  setMessage(addMessage, "");
  if (!addName.value.trim()) {
    // Suggest a name from the file name, e.g. "Minecraft Launcher.exe" -> "Minecraft Launcher".
    addName.value = path.split(/[\\/]/).pop().replace(/\.[^.]+$/, "");
  }
  addName.focus();
  addName.select();
});

for (const input of [addName, addArgs]) {
  input.addEventListener("input", () => setMessage(addMessage, ""));
}

document.getElementById("add-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!addPath.value) return setMessage(addMessage, "Choose a file first", true);
  if (!addName.value.trim()) return setMessage(addMessage, "Enter a name for the game", true);

  try {
    const game = await invoke("add_custom_game", {
      name: addName.value,
      path: addPath.value,
      args: addArgs.value,
    });
    setMessage(addMessage, `Added ${game.name}`);
    addPath.value = addName.value = addArgs.value = "";
    reloadShelf();
    refresh();
  } catch (err) {
    setMessage(addMessage, String(err), true);
  }
});

// ---------------------------------------------------------------- Window

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") currentWindow.close();
});

// Hidden games can change from the shelf while this window is open.
window.__TAURI__.event.listen("reload-shelf", () => refresh());

refresh();
