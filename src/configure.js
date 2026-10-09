import { invoke, convertFileSrc, el, setMessage, reloadShelf, SOURCE_LABELS } from "./common.js";

const currentWindow = window.__TAURI__.window.getCurrentWindow();

/** Set by the Rust side when it opens this window. */
const gameId = window.__GAME_ID__;

const configureTitle = document.getElementById("configure-title");
const configureSource = document.getElementById("configure-source");
const configurePreview = document.getElementById("configure-preview");
const configureMessage = document.getElementById("configure-message");
const renameInput = document.getElementById("rename-input");
const renameReset = document.getElementById("rename-reset");
const matchLabel = document.getElementById("match-label");
const matchAuto = document.getElementById("match-auto");
const matchQuery = document.getElementById("match-query");
const matchResults = document.getElementById("match-results");
const artTabs = [...document.querySelectorAll(".tabs button")];
const artOptions = document.getElementById("art-options");
const artHint = document.getElementById("art-hint");
const artTopRated = document.getElementById("art-top-rated");

const ART_HINTS = {
  cover: "The front of the box, shown when the game is pulled out.",
  hero: "The picture behind the logo on the spine.",
  logo: "The title drawn sideways on the spine.",
};

/** The game being configured, its cached art, the open tab and loaded options. */
const cfg = { game: null, art: null, kind: "cover", options: {} };

async function findGame(id) {
  const { games, hidden } = await invoke("list_games");
  return [...games, ...hidden].find((g) => g.id === id);
}

function describeMatch(game) {
  if (game.override.sgdbId) return `Matched to “${game.override.sgdbName ?? game.override.sgdbId}”`;
  if (game.source === "steam" && !game.override.name) return "Matched automatically by Steam app ID";
  return `Matched automatically by searching “${game.name}”`;
}

function render() {
  const { game, art } = cfg;
  currentWindow.setTitle(`Configure ${game.name}`);
  configureTitle.textContent = game.name;
  configureSource.textContent = `${SOURCE_LABELS[game.source] ?? game.source}${
    game.name !== game.originalName ? ` · originally “${game.originalName}”` : ""
  }`;
  configurePreview.hidden = !art?.cover;
  if (art?.cover) configurePreview.src = convertFileSrc(art.cover);
  if (document.activeElement !== renameInput) renameInput.value = game.name;
  if (document.activeElement !== matchQuery) matchQuery.value = game.name;
  renameReset.hidden = game.name === game.originalName;
  matchLabel.textContent = describeMatch(game);
  matchAuto.hidden = !game.override.sgdbId;
  const picked = game.override.cover || game.override.hero || game.override.logo;
  artTopRated.hidden = !picked;
  renderArtOptions();
}

/** Downloads the game's art again after a change, then updates this window and the shelf. */
async function refresh(message = "Updating artwork…") {
  setMessage(configureMessage, message);
  cfg.options = {};
  try {
    cfg.game = (await findGame(gameId)) ?? cfg.game;
    cfg.art = await invoke("get_artwork", { id: gameId });
    setMessage(configureMessage, cfg.art.cover || cfg.art.hero ? "" : "No artwork found for this match.");
  } catch (err) {
    setMessage(configureMessage, String(err), true);
  }
  reloadShelf();
  render();
}

/** Runs one change, showing errors in the window. */
async function configureAction(work, message) {
  try {
    await work();
  } catch (err) {
    setMessage(configureMessage, String(err), true);
    return;
  }
  await refresh(message);
}

function selectTab(kind) {
  cfg.kind = kind;
  for (const tab of artTabs) tab.setAttribute("aria-selected", String(tab.dataset.kind === kind));
  artHint.textContent = ART_HINTS[kind];
}

async function renderArtOptions() {
  const { kind } = cfg;
  artOptions.dataset.kind = kind;

  if (!cfg.options[kind]) {
    artOptions.replaceChildren(el("p", "empty", { textContent: "Loading…" }));
    cfg.options[kind] = invoke("artwork_options", { id: gameId, kind }).catch((err) => ({ error: String(err) }));
  }
  const options = await cfg.options[kind];
  if (cfg.kind !== kind) return; // switched tabs meanwhile

  if (options.error) {
    artOptions.replaceChildren(el("p", "empty", { textContent: options.error }));
    return;
  }
  if (options.length === 0) {
    artOptions.replaceChildren(
      el("p", "empty", { textContent: "SteamGridDB has nothing here. Try renaming the game or changing the match." }),
    );
    return;
  }

  // Art cached before source URLs were recorded is the top-rated, i.e. first, option.
  const picked = cfg.game.override[kind];
  const current = cfg.art?.[`${kind}Url`] ?? (picked || !cfg.art?.[kind] ? picked : options[0].url);
  artOptions.replaceChildren(
    ...options.map((option, i) => {
      const button = el("button", option.url === current ? "current" : "", { type: "button" });
      button.setAttribute("aria-label", `Use option ${i + 1}`);
      button.append(el("img", "", { src: option.thumb, alt: "", loading: "lazy" }));
      if (option.url === current) button.append(el("span", "current-tag", { textContent: "Current" }));
      button.addEventListener("click", () => {
        if (option.url === current) return;
        configureAction(() => invoke("choose_artwork", { id: gameId, kind, url: option.url }), "Downloading your pick…");
      });
      return button;
    }),
  );
}

for (const tab of artTabs) {
  tab.addEventListener("click", () => {
    selectTab(tab.dataset.kind);
    renderArtOptions();
  });
}

document.getElementById("rename-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const name = renameInput.value.trim();
  if (!name) return setMessage(configureMessage, "Enter a name", true);
  renameInput.blur();
  configureAction(() => invoke("rename_game", { id: gameId, name }), "Searching for artwork…");
});

renameReset.addEventListener("click", () => {
  renameInput.value = cfg.game.originalName;
  configureAction(() => invoke("rename_game", { id: gameId, name: "" }), "Searching for artwork…");
});

document.getElementById("match-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const query = matchQuery.value.trim();
  if (!query) return;
  matchResults.replaceChildren(el("li", "empty", { textContent: "Searching…" }));
  let matches;
  try {
    matches = await invoke("search_steamgriddb", { query });
  } catch (err) {
    matchResults.replaceChildren(el("li", "empty", { textContent: String(err) }));
    return;
  }
  if (matches.length === 0) {
    matchResults.replaceChildren(el("li", "empty", { textContent: "No games found. Try a shorter name." }));
    return;
  }
  matchResults.replaceChildren(
    ...matches.map((m) => {
      const label = m.year ? `${m.name} (${m.year})` : m.name;
      const button = el("button", "", { type: "button", textContent: label });
      button.addEventListener("click", () => {
        matchResults.replaceChildren();
        configureAction(
          () => invoke("set_game_match", { id: gameId, sgdbId: m.id, sgdbName: label }),
          "Downloading artwork for the new match…",
        );
      });
      const item = el("li");
      item.append(button);
      return item;
    }),
  );
});

matchAuto.addEventListener("click", () => {
  configureAction(() => invoke("set_game_match", { id: gameId, sgdbId: null, sgdbName: null }), "Searching for artwork…");
});

artTopRated.addEventListener("click", () => {
  configureAction(async () => {
    for (const kind of ["cover", "hero", "logo"]) {
      await invoke("choose_artwork", { id: gameId, kind, url: null });
    }
  }, "Downloading the top-rated artwork…");
});

document.getElementById("art-rescan").addEventListener("click", () => {
  configureAction(() => invoke("rescan_artwork", { id: gameId }), "Rescanning…");
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") currentWindow.close();
});

async function init() {
  selectTab("cover");
  cfg.game = await findGame(gameId);
  if (!cfg.game) {
    setMessage(configureMessage, "That game isn't installed anymore.", true);
    return;
  }
  render();
  renameInput.focus();
  try {
    cfg.art = await invoke("get_artwork", { id: gameId });
    render();
  } catch (err) {
    setMessage(configureMessage, String(err), true);
  }
}

init();
