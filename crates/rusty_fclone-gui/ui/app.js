// Vanilla JS frontend -- no bundler, no npm dependency. Recreates the
// design handoff (`Deduplication app UI design.zip`) against the real
// Tauri backend (crates/rusty_fclone-gui/src/commands.rs): every screen
// renders real data from `start_scan`/`run_action`/`find_duplicate_folders`,
// never mocked arrays. See docs/decisions/ADR-0022 for the specific
// places this deliberately deviates from the mockup (a real OS window
// instead of a fake titlebar, a single scan root instead of a folder
// list, disabled controls where no backend capability exists yet) and
// why.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const ACTION_KINDS = [
  { id: "trash", label: "Trash" },
  { id: "delete", label: "Delete" },
  { id: "hardlink", label: "Hardlink" },
  { id: "reflink", label: "Reflink" },
  { id: "move", label: "Move" },
  { id: "copy", label: "Copy" },
];

// "move"/"copy" (ACTION-MOVE-COPY, ADR-0026) are the only two kinds that
// need a destination folder -- shown as an extra field in the review
// action bar only when one of them is selected, and required before Apply
// is enabled for either.
const ARCHIVE_KINDS = new Set(["move", "copy"]);

const KIND_COLOR = {
  photo: "var(--accent)",
  video: "var(--purple)",
  document: "var(--warning)",
  audio: "var(--success)",
  archive: "var(--pink)",
  other: "var(--other-gray)",
};

const TYPE_FILTERS = [
  { id: "photo", label: "Photos" },
  { id: "video", label: "Videos" },
  { id: "document", label: "Documents" },
  { id: "audio", label: "Audio" },
  { id: "archive", label: "Archives" },
];

const EXT_CATEGORY = {
  jpg: "photo", jpeg: "photo", png: "photo", gif: "photo", webp: "photo",
  heic: "photo", bmp: "photo", tiff: "photo", tif: "photo", svg: "photo",
  mp4: "video", mov: "video", avi: "video", mkv: "video", webm: "video", m4v: "video",
  pdf: "document", doc: "document", docx: "document", xls: "document",
  xlsx: "document", ppt: "document", pptx: "document", txt: "document",
  md: "document", csv: "document", rtf: "document", odt: "document",
  mp3: "audio", wav: "audio", flac: "audio", m4a: "audio", aac: "audio", ogg: "audio",
  zip: "archive", tar: "archive", gz: "archive", "7z": "archive", rar: "archive",
  bz2: "archive", xz: "archive",
};

function categoryOf(path) {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return "other";
  const ext = path.slice(dot + 1).toLowerCase();
  return EXT_CATEGORY[ext] || "other";
}

function fileNameOf(path) {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

// ---- real path helpers (GUI-FS-BROWSE / GUI-REVIEW-PANELS) ---------------
// Every path handled here is a real absolute filesystem path the backend
// already returned (a duplicate group's member, a folder match, a
// `list_directory` entry) -- never a fabricated one, unlike the design
// handoff's static mocked tree.

function normPath(path) {
  return (path || "").replace(/\\/g, "/").replace(/\/+$/, "") || "/";
}

// The directory containing `path` -- i.e. `path` with its final segment
// removed, still an absolute path.
function parentOf(path) {
  const norm = normPath(path);
  const idx = norm.lastIndexOf("/");
  return idx <= 0 ? "/" : norm.slice(0, idx);
}

// True if `path` is `dir` itself or lives somewhere underneath it.
function pathUnder(path, dir) {
  const p = normPath(path);
  const d = normPath(dir);
  return p === d || p.startsWith(d + "/");
}

// `path`'s directory chain relative to `root` (e.g. `/a/b/c/file.txt` under
// root `/a` is `["b", "c"]`) -- falls back to the full absolute chain if
// `path` isn't actually under `root`.
function relativeChain(path, root) {
  const dir = parentOf(path);
  const r = normPath(root || "/");
  if (dir === r) return [];
  if (dir.startsWith(r + "/")) return dir.slice(r.length + 1).split("/").filter(Boolean);
  return normPath(dir).split("/").filter(Boolean);
}

// Resolves (via `list_directory`) the real immediate subdirectories of
// `path` (or the platform's browse roots, when `path` is falsy), caching
// the result in `cache` keyed by `path || ""` -- shared across every panel
// showing that directory. Same in-flight-marker pattern as `ensurePreview`:
// mutates `cache`/`open` directly for the `null` "in flight" marker to
// avoid a re-render mid-render, then calls `onLoaded` (expected to trigger
// a re-render) once the real children come back.
function ensureDirChildren(path, cache, onLoaded) {
  const key = path || "";
  if (key in cache) return;
  cache[key] = null;
  invoke("list_directory", { path: path || null })
    .then((entries) => {
      cache[key] = entries;
      onLoaded();
    })
    .catch(() => {
      cache[key] = [];
      onLoaded();
    });
}

function bytesHuman(n) {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

function relativeTime(ms) {
  const diff = Date.now() - ms;
  const min = Math.floor(diff / 60000);
  if (min < 1) return "Just now";
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  return `${days}d ago`;
}

// ---- state -------------------------------------------------------------

const state = {
  theme: "dark",
  view: "dashboard",
  // Sidebar collapse (GUI-REVIEW-PANELS): a 64px icon-only rail, toggled
  // from a button at the sidebar's own base -- frees width for Duplicate
  // Review's three side-by-side panels on a narrow window. Session-only,
  // like every other render-only UI preference here (theme included).
  navCollapsed: false,
  scanRoot: "",
  options: {
    followSymlinks: false,
    crossFilesystems: false,
    verifyMatches: false,
    ioThreads: null,
    cachePath: "",
    fclonesImportPath: "",
    minSize: "",
    maxSize: "",
    includeExtensions: "",
    excludeExtensions: "",
    excludePaths: "",
  },
  matchMode: "exact",
  typeFilter: new Set(TYPE_FILTERS.map((t) => t.id)),
  // Saved scan profiles (SCAN-PROFILES, ADR-0029) -- loaded once at
  // startup from the backend's persisted JSON file (`list_scan_profiles`)
  // and kept in sync with every save/delete's response, so this is always
  // the full current list rather than something re-fetched per render.
  scanProfiles: [],
  profileNameInput: "",
  profileError: null,
  scanning: false,
  scanError: null,
  progress: { filesScanned: 0, bytesScanned: 0 },
  groups: [],
  folderMatches: [],
  findingFolders: false,
  // Perceptual "similar images" clusters (DETECTION-PERCEPTUAL-IMAGES,
  // ADR-0030) -- only populated when `matchMode === "similar"`; a
  // deliberately separate, opt-in, report-only pass run after a normal
  // scan finishes, never merged into `groups`, which stays exclusively
  // byte-identical duplicates.
  similarGroups: [],
  findingSimilarImages: false,
  lastSummary: null,
  scanHistory: [],
  errors: [],
  groupIndex: 0,
  // Duplicate Review's real file-system panel (GUI-REVIEW-PANELS) -- a
  // lazily-expanded tree of the scan root's real subdirectories, backed by
  // the `list_directory` command. `fsChildrenCache` maps an absolute path
  // to its already-fetched DirEntryPayload[] children (`null` while a
  // fetch is in flight, absent if never requested); `fsOpenPaths` is the
  // set of expanded directories. Reset whenever a new scan starts (see
  // `startScan`), since a prior scan's tree is meaningless for a new root.
  fsChildrenCache: {},
  fsOpenPaths: new Set(),
  // Clicking a File System panel row filters the duplicate-group panel and
  // the Group X of N navigation to items located under this real absolute
  // path; `null` means no filter.
  fsSelectedPath: null,
  fsTreeCollapsed: false,
  // Duplicate-group panel's nested folder sections (grouped by real path
  // hierarchy under the scan root) that are collapsed, keyed by the
  // relative chain joined with "/".
  dupCollapsedFolders: new Set(),
  dupTreeCollapsed: false,
  // Inline media preview (GUI-MEDIA-PREVIEW, ADR-0028) -- keyed by file
  // path, value is `null` (loading), a data: URI (loaded), or `false`
  // (not previewable: unsupported type, too large, or a read error).
  // Session-scoped like every other render cache here; never persisted.
  previewCache: {},
  keepChoice: {},
  keepRule: "alphabetical",
  ruleKeepChoice: {},
  // Comma-separated protected/reference folders (ACTION-REFERENCE-FOLDERS,
  // ADR-0025) -- any file under one of these is never planned or acted on,
  // and always wins as the "keep" over `keepRule`. Kept separate from
  // `state.options` since it isn't a scan tunable: detection (`start_scan`,
  // `find_duplicate_folders`) doesn't take it, only the action commands do.
  referencePaths: "",
  actionKind: "trash",
  // Destination folder for "move"/"copy" (ACTION-MOVE-COPY, ADR-0026) --
  // only meaningful, and required, when actionKind is one of those two.
  archiveDir: "",
  sessionBytesReclaimed: 0,
  actionMessage: null,
  // Scan Setup's "Browse..." folder picker (GUI-FS-BROWSE) -- a real,
  // lazily-expanded tree of the machine's actual directories, backed by
  // `list_directory`. Same cache/open-set shape as the Review file-system
  // panel above, but rooted at the platform's browse starting points
  // (`list_directory(null)`) instead of a scan root, since here the user
  // is picking one.
  browseModalOpen: false,
  browseChildrenCache: {},
  browseOpenPaths: new Set(),
  browseSelectedPath: null,
  rules: [
    {
      id: 1,
      title: "Keep newest copy",
      desc: "When duplicates are found, prefer keeping the most recently modified file.",
      enabled: true,
    },
    {
      id: 2,
      title: "Ignore tiny files",
      desc: "Skip files smaller than 10 KB -- thumbnails, icons, temp files.",
      enabled: true,
    },
    {
      id: 3,
      title: "Auto-clean Downloads",
      desc: "After each scan, automatically remove duplicates found in Downloads.",
      enabled: false,
    },
  ],
};

function setState(patch) {
  Object.assign(state, patch);
  render();
}

// ---- tiny DOM helper -----------------------------------------------------

// Builds an element without ever routing untrusted data (file paths, error
// messages) through innerHTML -- string children become text nodes.
function el(tag, props, ...children) {
  const node = document.createElement(tag);
  props = props || {};
  for (const [key, value] of Object.entries(props)) {
    if (value == null || value === false) continue;
    if (key === "className") node.className = value;
    else if (key.startsWith("on") && key.length > 2 && typeof value === "function") {
      // Generic DOM event wiring -- onClick, onMouseEnter, onFocus, etc. --
      // so a new interaction (e.g. a chart segment's hover tooltip) doesn't
      // need a bespoke branch here every time one is needed.
      node.addEventListener(key.slice(2).toLowerCase(), value);
    }
    else if (key === "disabled") node.disabled = true;
    else if (key === "type") node.type = value;
    else if (key === "placeholder") node.placeholder = value;
    else if (key === "value") node.value = value;
    else if (key === "checked") node.checked = value;
    else if (key === "title") node.title = value;
    else node.setAttribute(key, value);
  }
  for (const child of children.flat()) {
    if (child == null || child === false) continue;
    node.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
  }
  return node;
}

// ---- root -------------------------------------------------------------

const app = document.getElementById("app");

// A single shared hover/focus tooltip for chart segments (e.g. the
// Dashboard's storage-breakdown bar) -- appended to <body>, a sibling of
// #app, so it survives `render()`'s wholesale rebuild of #app's children
// instead of needing to be recreated (and re-shown) on every state change.
// Positioned and shown/hidden imperatively, the same "bypass render() for
// something that shouldn't trigger a full rebuild" precedent `pathInput`
// already established for keystroke input.
const chartTooltip = el("div", { className: "chart-tooltip" });
document.body.appendChild(chartTooltip);

function showChartTooltip(target, primaryText, secondaryText) {
  chartTooltip.replaceChildren(
    el("div", { className: "chart-tooltip-value" }, primaryText),
    el("div", { className: "chart-tooltip-label" }, secondaryText),
  );
  const rect = target.getBoundingClientRect();
  chartTooltip.style.left = `${rect.left + rect.width / 2}px`;
  chartTooltip.style.top = `${rect.top}px`;
  chartTooltip.classList.add("visible");
}

function hideChartTooltip() {
  chartTooltip.classList.remove("visible");
}

function render() {
  document.documentElement.setAttribute("data-theme", state.theme);
  app.replaceChildren(sidebar(), content());
}

function sidebar() {
  const collapsed = state.navCollapsed;
  const labels = ["dashboard", "scan", "review", "rules"];
  const titles = { dashboard: "Dashboard", scan: "Scan", review: "Review", rules: "Rules" };
  const navItem = (id, view) =>
    el(
      "button",
      {
        className: "nav-item" + (state.view === view ? " active" : ""),
        onClick: () => setState({ view }),
        title: collapsed ? titles[id] : null,
      },
      icon(id, 17),
      !collapsed && el("span", null, titles[id]),
    );

  return el(
    "div",
    { className: "sidebar" + (collapsed ? " collapsed" : "") },
    el(
      "div",
      null,
      el(
        "div",
        { className: "brand" },
        el("div", { className: "brand-icon" }, icon("logo", 15)),
        !collapsed && el("div", { className: "brand-name" }, "Rusty FClone"),
      ),
      el("div", { className: "nav" }, ...labels.map((id) => navItem(id, id))),
    ),
    el(
      "div",
      null,
      el("div", { className: "sidebar-footer-divider" }),
      el(
        "button",
        { className: "theme-toggle", onClick: toggleTheme, title: collapsed ? (state.theme === "dark" ? "Light mode" : "Dark mode") : null },
        icon(state.theme === "dark" ? "sun" : "moon", 15),
        !collapsed && el("span", null, state.theme === "dark" ? "Light mode" : "Dark mode"),
      ),
      !collapsed && el(
        "div",
        { className: "session-savings" },
        el("div", { className: "session-savings-label" }, "Reclaimed this session"),
        el("div", { className: "session-savings-value" }, bytesHuman(state.sessionBytesReclaimed)),
      ),
      el(
        "button",
        {
          className: "sidebar-collapse-btn",
          onClick: () => setState({ navCollapsed: !collapsed }),
          title: collapsed ? "Expand sidebar" : "Collapse sidebar",
        },
        icon(collapsed ? "chevronRight" : "chevronLeft", 15),
        !collapsed && el("span", null, "Collapse"),
      ),
    ),
  );
}

function toggleTheme() {
  setState({ theme: state.theme === "dark" ? "light" : "dark" });
}

function content() {
  const view =
    state.view === "dashboard" ? dashboardView() :
    state.view === "scan" ? scanView() :
    state.view === "review" ? reviewView() :
    rulesView();
  return el("div", { className: "content" }, view);
}

// ---- dashboard ----------------------------------------------------------

function dashboardView() {
  const totalReclaim = state.groups.reduce(
    (sum, g) => sum + g.size * Math.max(g.paths.length - 1, 0),
    0,
  );
  const duplicateFiles = state.lastSummary ? state.lastSummary.duplicateFiles : 0;

  const breakdown = storageBreakdown();

  return el(
    "div",
    { className: "view" },
    el(
      "div",
      { className: "view-header" },
      el(
        "div",
        null,
        el("div", { className: "view-title" }, "Overview"),
        el("div", { className: "view-subtitle" }, "Real results from scans run this session."),
      ),
      el("button", { className: "btn btn-primary", onClick: () => setState({ view: "scan" }) }, "New Scan"),
    ),
    el(
      "div",
      { className: "stat-row" },
      el(
        "div",
        { className: "card stat-card" },
        el("div", { className: "stat-label" }, "Duplicates found"),
        el("div", { className: "stat-value" }, `${duplicateFiles} `, el("span", { className: "unit" }, "files")),
      ),
      el(
        "div",
        { className: "card stat-card" },
        el("div", { className: "stat-label" }, "Space to reclaim (est.)"),
        el("div", { className: "stat-value success" }, bytesHuman(totalReclaim)),
      ),
      el(
        "div",
        { className: "card stat-card" },
        el("div", { className: "stat-label" }, "Last scan"),
        el(
          "div",
          { className: "stat-value" },
          state.lastSummary ? relativeTime(state.lastSummary.finishedAt) : "Never",
        ),
      ),
    ),
    el(
      "div",
      { className: "card" },
      el("div", { className: "card-title" }, "Storage breakdown"),
      el(
        "div",
        { className: "breakdown-bar", style: "margin-top:14px" },
        ...breakdown.map((b) =>
          el("button", {
            className: "breakdown-segment",
            style: `width:${b.pct}%;background:${b.color}`,
            "aria-label": `${b.label}, ${bytesHuman(b.bytes)}, ${b.pct} percent`,
            onMouseEnter: (e) => showChartTooltip(e.currentTarget, bytesHuman(b.bytes), `${b.label} · ${b.pct}%`),
            onFocus: (e) => showChartTooltip(e.currentTarget, bytesHuman(b.bytes), `${b.label} · ${b.pct}%`),
            onMouseLeave: hideChartTooltip,
            onBlur: hideChartTooltip,
          }),
        ),
      ),
      el(
        "div",
        { className: "breakdown-legend" },
        ...(breakdown.length
          ? breakdown.map((b) =>
              el(
                "div",
                { className: "breakdown-legend-item" },
                el("div", { className: "legend-dot", style: `background:${b.color}` }),
                `${b.label} · ${bytesHuman(b.bytes)} (${b.pct}%)`,
              ),
            )
          : [el("div", { className: "breakdown-legend-item" }, "No duplicate files yet.")]),
      ),
    ),
    recentScansCard(),
  );
}

function storageBreakdown() {
  const byCategory = {};
  let total = 0;
  for (const g of state.groups) {
    const redundant = Math.max(g.paths.length - 1, 0);
    if (redundant <= 0) continue;
    const cat = categoryOf(g.paths[0]);
    byCategory[cat] = (byCategory[cat] || 0) + g.size * redundant;
    total += g.size * redundant;
  }
  if (total === 0) return [];
  const labels = { photo: "Photos", video: "Videos", document: "Documents", audio: "Audio", archive: "Archives", other: "Other" };
  return Object.entries(byCategory)
    .sort((a, b) => b[1] - a[1])
    .map(([cat, bytes]) => ({
      label: labels[cat] || "Other",
      bytes,
      pct: Math.round((bytes / total) * 100),
      color: KIND_COLOR[cat] || KIND_COLOR.other,
    }));
}

function recentScansCard() {
  return el(
    "div",
    { className: "card" },
    el(
      "div",
      { className: "card-header-row" },
      el("div", { className: "card-title" }, "Recent scans"),
      el(
        "div",
        { style: "display:flex;gap:8px" },
        el(
          "button",
          { className: "btn btn-ghost", disabled: true, title: "Reading a CLI --history SQLite database needs a real filesystem path from a native file picker, which isn't wired into the GUI yet (blocked on Tauri's dialog/fs plugin work)" },
          icon("dashboard", 13),
          "Import history",
        ),
        el(
          "button",
          {
            className: "btn btn-ghost",
            disabled: state.scanHistory.length === 0,
            title: state.scanHistory.length === 0
              ? "No scans run yet this session"
              : "Download this session's scan history as a JSON file",
            onClick: exportScanHistoryJson,
          },
          icon("dashboard", 13),
          "Export (JSON)",
        ),
      ),
    ),
    state.scanHistory.length === 0
      ? el("div", { className: "empty-note" }, "No scans run yet this session. Recent scans aren't saved between launches -- use the CLI's --history flag for persistent tracking.")
      : el(
          "div",
          { className: "scan-table" },
          el(
            "div",
            { className: "scan-table-head" },
            el("div", null, "Location"),
            el("div", null, "Date"),
            el("div", null, "Duplicates"),
            el("div", null, "Reclaimable"),
            el("div", null, "Status"),
          ),
          ...state.scanHistory.map((h) =>
            el(
              "div",
              { className: "scan-table-row" },
              el("div", { className: "loc" }, fileNameOf(h.root) || h.root),
              el("div", { className: "date" }, relativeTime(h.finishedAt)),
              el("div", null, String(h.duplicateFiles)),
              el("div", { style: "color:var(--success)" }, bytesHuman(h.reclaimEstimate)),
              el("div", null, el("span", { className: "badge", style: "background:var(--success-tint);color:var(--success)" }, "Complete")),
            ),
          ),
        ),
  );
}

// ---- scan setup -----------------------------------------------------------

function scanView() {
  const opt = state.options;
  const toggle = (label, key) =>
    el(
      "div",
      { className: "toggle-row" },
      el("span", { className: "toggle-row-label" }, label),
      el(
        "button",
        { className: "toggle-track" + (opt[key] ? " on" : ""), onClick: () => setState({ options: { ...opt, [key]: !opt[key] } }) },
        el("div", { className: "toggle-knob" }),
      ),
    );

  return el(
    "div",
    { className: "view", style: "position:relative" },
    el(
      "div",
      { className: "view-header" },
      el(
        "div",
        null,
        el("div", { className: "view-title" }, "Scan Setup"),
        el("div", { className: "view-subtitle" }, "Choose a location and matching options."),
      ),
    ),
    profilesCard(),
    el(
      "div",
      { className: "scan-layout" },
      el(
        "div",
        { className: "card scan-main" },
        el("div", { className: "card-title" }, "Folder to scan"),
        el("div", {
          className: "field-label",
          style: "margin-top:10px;margin-bottom:6px",
        }, "Directory"),
        el(
          "div",
          { style: "display:flex;gap:8px" },
          el("div", { style: "flex:1;min-width:0" }, pathInput("e.g. /home/me/Pictures", state.scanRoot, (v) => { state.scanRoot = v; })),
          el("button", { className: "btn btn-ghost", onClick: openBrowseModal }, "Browse…"),
        ),
        el("div", { className: "hint" }, "Scanning multiple locations at once isn't supported yet -- enter one root directory."),
      ),
      el(
        "div",
        { className: "scan-side" },
        el(
          "div",
          { className: "card" },
          el("div", { className: "card-title" }, "Match sensitivity"),
          el(
            "div",
            { className: "seg" },
            el(
              "button",
              { className: "seg-option" + (state.matchMode === "exact" ? " active" : ""), onClick: () => setState({ matchMode: "exact" }) },
              "Exact match",
            ),
            el(
              "button",
              { className: "seg-option" + (state.matchMode === "similar" ? " active" : ""), onClick: () => setState({ matchMode: "similar" }) },
              "Similar content",
            ),
          ),
          state.matchMode === "similar"
            ? el(
                "div",
                { className: "hint" },
                "Runs alongside the exact scan, not instead of it -- similar (not byte-identical) images show up in Review as their own, separately-labeled clusters, with no action offered (DETECTION-PERCEPTUAL-IMAGES).",
              )
            : null,
        ),
        el(
          "div",
          { className: "card" },
          el("div", { className: "card-title" }, "Show in review"),
          el(
            "div",
            { style: "display:flex;flex-wrap:wrap;gap:8px" },
            ...TYPE_FILTERS.map((t) =>
              el(
                "button",
                {
                  className: "chip" + (state.typeFilter.has(t.id) ? " active" : ""),
                  onClick: () => {
                    const next = new Set(state.typeFilter);
                    next.has(t.id) ? next.delete(t.id) : next.add(t.id);
                    setState({ typeFilter: next });
                  },
                },
                t.label,
              ),
            ),
          ),
          el("div", { className: "hint" }, "Filters what's shown in Review after a scan -- doesn't change what's scanned."),
        ),
        el(
          "div",
          { className: "card", style: "display:flex;flex-direction:column;gap:14px" },
          el("div", { className: "card-title" }, "Scan options"),
          toggle("Follow symlinks", "followSymlinks"),
          toggle("Cross filesystems", "crossFilesystems"),
          toggle("Byte-verify matches", "verifyMatches"),
        ),
      ),
    ),
    el(
      "div",
      { className: "card" },
      el("div", { className: "card-title", style: "margin-bottom:4px" }, "Data & caching"),
      el("div", { className: "hint", style: "margin-bottom:14px" }, "Speed up re-scans, or reuse hashes already computed by another tool."),
      el(
        "div",
        { className: "field-row" },
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Hash cache path"),
          pathInput("(none) -- e.g. ~/.cache/rusty-fclone.db", opt.cachePath, (v) => { state.options.cachePath = v; }),
        ),
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Import fclones cache"),
          pathInput("(none) -- import an existing fclones cache", opt.fclonesImportPath, (v) => { state.options.fclonesImportPath = v; }),
        ),
      ),
    ),
    el(
      "div",
      { className: "card" },
      el("div", { className: "card-title", style: "margin-bottom:4px" }, "Include/exclude filters"),
      el("div", { className: "hint", style: "margin-bottom:14px" }, "Applied during the scan itself -- a filtered-out file is never read or hashed."),
      el(
        "div",
        { className: "field-row" },
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Min size (bytes)"),
          pathInput("(none)", opt.minSize, (v) => { state.options.minSize = v; }),
        ),
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Max size (bytes)"),
          pathInput("(none)", opt.maxSize, (v) => { state.options.maxSize = v; }),
        ),
      ),
      el(
        "div",
        { className: "field-row" },
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Only these extensions"),
          pathInput("(none) -- e.g. jpg, png, heic", opt.includeExtensions, (v) => { state.options.includeExtensions = v; }),
        ),
        el(
          "div",
          { className: "field-col" },
          el("div", { className: "field-label" }, "Skip these extensions"),
          pathInput("(none) -- e.g. tmp, log", opt.excludeExtensions, (v) => { state.options.excludeExtensions = v; }),
        ),
      ),
      el(
        "div",
        { className: "field-col" },
        el("div", { className: "field-label" }, "Skip these folders (comma-separated)"),
        pathInput("(none) -- e.g. /home/me/node_modules, /home/me/.cache", opt.excludePaths, (v) => { state.options.excludePaths = v; }),
      ),
    ),
    el(
      "div",
      { className: "card" },
      el("div", { className: "card-title", style: "margin-bottom:4px" }, "Protected folders"),
      el(
        "div",
        { className: "hint", style: "margin-bottom:14px" },
        "A file under any of these is never deleted, trashed, hardlinked, or reflinked -- it's always the one kept, in every duplicate group it appears in.",
      ),
      el(
        "div",
        { className: "field-col" },
        el("div", { className: "field-label" }, "Never touch files under (comma-separated)"),
        pathInput("(none) -- e.g. /home/me/originals, /home/me/archive", state.referencePaths, (v) => { state.referencePaths = v; }),
      ),
    ),
    el(
      "div",
      { className: "card scan-footer" },
      el(
        "div",
        { className: "hint" },
        state.scanError ? el("span", { className: "error-text" }, state.scanError) : "Enter a directory, then start the scan.",
      ),
      el(
        "button",
        { className: "btn btn-primary large", disabled: state.scanning, onClick: startScan },
        state.scanning ? "Scanning..." : "Start Scan",
      ),
    ),
    state.browseModalOpen && browseModal(),
  );
}

// A real, lazily-expanded directory tree for picking a scan root
// (GUI-FS-BROWSE) -- opened from Scan Setup's "Browse..." button. Backed
// by the `list_directory` command, starting from the platform's browse
// roots (home directory, plus `/` or drive letters). This replaces the
// design handoff's static mocked filesystem tree with a real one; see
// `payload::DirEntryPayload`'s doc comment for why.
function openBrowseModal() {
  ensureDirChildren(null, state.browseChildrenCache, () => render());
  setState({ browseModalOpen: true, browseSelectedPath: null });
}

function browseModal() {
  const rows = [];
  const walk = (path, depth) => {
    const children = state.browseChildrenCache[path || ""];
    if (!children) return;
    for (const entry of children) {
      rows.push({ entry, depth });
      if (state.browseOpenPaths.has(entry.path)) walk(entry.path, depth + 1);
    }
  };
  walk(null, 0);

  const loading = state.browseChildrenCache[""] === null;

  return el(
    "div",
    { className: "modal-overlay" },
    el(
      "div",
      { className: "modal-card" },
      el("div", { className: "card-title" }, "Choose a folder"),
      el("div", { className: "hint", style: "margin-bottom:12px" }, "No native file picker yet -- browse the real directory tree below."),
      el(
        "div",
        { className: "browse-tree" },
        loading
          ? el("div", { className: "empty-note" }, "Loading…")
          : rows.length === 0
            ? el("div", { className: "empty-note" }, "No accessible folders found.")
            : rows.map((row) => browseRow(row)),
      ),
      el("div", { className: "hint", style: "margin:12px 0" }, `Selected: ${state.browseSelectedPath || "No folder selected"}`),
      el(
        "div",
        { style: "display:flex;gap:10px;justify-content:flex-end" },
        el("button", { className: "btn btn-ghost", onClick: () => setState({ browseModalOpen: false }) }, "Cancel"),
        el(
          "button",
          {
            className: "btn btn-primary",
            disabled: !state.browseSelectedPath,
            onClick: () => setState({ scanRoot: state.browseSelectedPath, browseModalOpen: false }),
          },
          "Select Folder",
        ),
      ),
    ),
  );
}

function browseRow({ entry, depth }) {
  const expanded = state.browseOpenPaths.has(entry.path);
  const selected = state.browseSelectedPath === entry.path;
  return el(
    "div",
    { className: "tree-row" + (selected ? " active" : ""), style: `padding-left:${8 + depth * 16}px`, onClick: () => setState({ browseSelectedPath: entry.path }) },
    entry.hasChildren
      ? el("span", {
          className: "tree-chevron",
          onClick: (e) => {
            e.stopPropagation();
            const open = new Set(state.browseOpenPaths);
            if (open.has(entry.path)) {
              open.delete(entry.path);
            } else {
              open.add(entry.path);
              ensureDirChildren(entry.path, state.browseChildrenCache, () => render());
            }
            setState({ browseOpenPaths: open });
          },
        }, expanded ? "▾" : "▸")
      : el("span", { className: "tree-chevron" }),
    icon("folder", 13),
    el("span", { className: "tree-row-label", title: entry.path }, entry.name),
  );
}

// Saved scan profiles (SCAN-PROFILES, ADR-0029): the current root + scan
// options, saved under a name and re-loadable across launches -- upgrades
// the prior session-only "Recent scans" list (still shown on Dashboard,
// unrelated) into something persisted. Placed above the scan-layout grid so
// it's the first thing seen on Scan Setup, the same way `recentScansCard`
// leads the Dashboard.
function profilesCard() {
  return el(
    "div",
    { className: "card" },
    el(
      "div",
      { className: "card-header-row" },
      el("div", { className: "card-title" }, "Saved scan profiles"),
    ),
    el("div", { className: "hint", style: "margin-bottom:14px" }, "Save the current directory and options as a named preset, re-runnable across launches."),
    el(
      "div",
      { className: "field-row" },
      el(
        "div",
        { className: "field-col" },
        el("div", { className: "field-label" }, "Profile name"),
        pathInput("e.g. Downloads cleanup", state.profileNameInput, (v) => { state.profileNameInput = v; }),
      ),
      el(
        "div",
        { style: "display:flex;align-items:flex-end" },
        el("button", { className: "btn btn-ghost", onClick: saveScanProfile }, "Save current setup"),
      ),
    ),
    state.profileError && el("div", { className: "hint error-text" }, state.profileError),
    state.scanProfiles.length === 0
      ? el("div", { className: "empty-note" }, "No saved profiles yet.")
      : el(
          "div",
          { className: "profile-list" },
          ...state.scanProfiles.map((p) =>
            el(
              "div",
              { className: "profile-row" },
              el(
                "div",
                null,
                el("div", { className: "profile-row-name" }, p.name),
                el("div", { className: "hint" }, p.root),
              ),
              el(
                "div",
                { style: "display:flex;gap:8px" },
                el("button", { className: "btn btn-ghost", onClick: () => applyScanProfile(p) }, "Load"),
                el("button", { className: "btn btn-danger", onClick: () => deleteScanProfile(p.name) }, "Delete"),
              ),
            ),
          ),
        ),
  );
}

// A text input that mutates state directly on every keystroke without
// triggering render() -- a full re-render replaces the DOM (see `render`),
// which would drop focus/cursor position after every character if these
// were wired through setState like everything else. `onInput` should
// mutate `state` in place and return nothing; the field's displayed value
// is picked up fresh on the next render triggered by something else
// (a toggle, nav, or the Start Scan click).
function pathInput(placeholder, value, onInput) {
  const input = el("input", { className: "text-input", type: "text", placeholder, value });
  input.addEventListener("input", (e) => onInput(e.target.value));
  return input;
}

// ---- review -----------------------------------------------------------

// The real absolute path a review item is "at" -- a file group's kept
// candidate, a folder match's primary folder, or a similar-images
// cluster's first member. Used both to color/filter the real file-system
// panel and to place the item in the duplicate-group panel's nested tree.
function itemRepresentativePath(item) {
  if (item.kind === "folder") {
    return item.match.type === "exact" ? item.match.folders[0] : item.match.subset;
  }
  return item.group.paths[0];
}

// Every real copy `item` has, not just its representative one -- e.g. a
// file group's every path, or an exact folder match's every folder. Used
// by the file-system panel's direct-duplicate badges, so a folder shows a
// badge as soon as *any* copy of *any* duplicate lives there, the same
// breadth the design handoff's own mocked badges used.
function itemAllPaths(item) {
  if (item.kind === "folder") {
    return item.match.type === "exact" ? item.match.folders : [item.match.subset, item.match.superset];
  }
  return item.group.paths;
}

// `item`'s real directory chain relative to the scanned root (GUI-REVIEW-
// PANELS) -- e.g. an item at `<root>/Documents/Finance/report.pdf` chains
// to `["Documents", "Finance"]`. Drives the duplicate-group panel's nested
// folder sections, mirroring the real path hierarchy instead of a flat list.
function itemChain(item) {
  return relativeChain(itemRepresentativePath(item), state.scanRoot);
}

function itemColor(item) {
  if (item.kind === "folder") return "var(--pink)";
  if (item.kind === "similar") return "var(--warning)";
  return KIND_COLOR[categoryOf(item.group.paths[0])] || "var(--accent)";
}

function itemName(item) {
  return fileNameOf(itemRepresentativePath(item));
}

function itemMeta(item) {
  if (item.kind === "folder") return `${item.match.fileCount} files · ${bytesHuman(item.match.bytes)}`;
  if (item.kind === "similar") return `${item.group.paths.length} similar · max diff ${item.group.maxDistance}/64`;
  return `${item.group.paths.length} copies · ${bytesHuman(item.group.size)}`;
}

function colorTint(cssVar) {
  // Every KIND_COLOR entry is a var(--token) reference; the *-tint custom
  // properties already exist for accent/success/danger/pink, but per-kind
  // tints (purple/warning/other) don't have a dedicated variable, so tint
  // generically via color-mix, which every target webview (WebKitGTK,
  // WebView2, WKWebView) supports.
  return `color-mix(in srgb, ${cssVar} 16%, transparent)`;
}

// The type-filtered review items, before the file-system panel's folder
// filter is applied -- used by the file-system panel itself so its "which
// folders hold duplicates" badges never disappear just because a filter is
// currently narrowing the other two panels.
function baseReviewItems() {
  const files = state.groups
    .filter((g) => state.typeFilter.has(categoryOf(g.paths[0])))
    .map((g, i) => ({ kind: "file", group: g, key: `file-${i}` }));
  const folders = state.folderMatches.map((m, i) => ({ kind: "folder", match: m, key: `folder-${i}` }));
  const similar = state.similarGroups.map((g, i) => ({ kind: "similar", group: g, key: `similar-${i}` }));
  return files.concat(folders).concat(similar);
}

// The items actually shown/navigated in Review: type-filtered, further
// narrowed by the file-system panel's selected folder (if any), and sorted
// by real directory chain so this list's order always matches the
// duplicate-group panel's nested tree order.
function reviewItems() {
  const base = baseReviewItems();
  const filtered = state.fsSelectedPath
    ? base.filter((item) => pathUnder(itemRepresentativePath(item), state.fsSelectedPath))
    : base;
  return filtered
    .slice()
    .sort((a, b) => itemChain(a).join("/").localeCompare(itemChain(b).join("/")) || itemName(a).localeCompare(itemName(b)));
}

function scanningSubtitle() {
  return `Scanning... ${state.progress.filesScanned} files, ${bytesHuman(state.progress.bytesScanned)} scanned so far.`;
}

function errorsPanel() {
  if (state.errors.length === 0) return null;
  return el(
    "div",
    { className: "card", style: "flex-shrink:0;border-color:var(--danger)" },
    el("div", { className: "card-title", style: "color:var(--danger)" }, `${state.errors.length} file error${state.errors.length === 1 ? "" : "s"} during scan`),
    ...state.errors.slice(0, 5).map((e) => el("div", { className: "hint error-text" }, `${e.path}: ${e.message}`)),
    state.errors.length > 5 && el("div", { className: "hint" }, `+${state.errors.length - 5} more`),
  );
}

function reviewView() {
  const allBase = baseReviewItems();
  const items = reviewItems();

  if (allBase.length === 0) {
    return el(
      "div",
      { className: "view" },
      viewHeader("Duplicate Review", state.scanning ? scanningSubtitle() : "Step through duplicate groups and resolve each."),
      errorsPanel(),
      el(
        "div",
        { className: "empty-state" },
        icon("scanEmpty", 40),
        el("div", null, state.scanning ? "Waiting for the first duplicate group..." : "No duplicates to review yet."),
        !state.scanning && el("button", { className: "btn btn-primary", onClick: () => setState({ view: "scan" }) }, "Run a scan"),
      ),
    );
  }

  const idx = Math.min(state.groupIndex, Math.max(items.length - 1, 0));
  const current = items[idx];
  const clearFilter = () => setState({ fsSelectedPath: null, groupIndex: 0 });

  return el(
    "div",
    { className: "view" },
    el(
      "div",
      { className: "view-header" },
      el(
        "div",
        null,
        el("div", { className: "view-title" }, "Duplicate Review"),
        el("div", { className: "view-subtitle" }, state.scanning ? scanningSubtitle() : `${items.length} of ${allBase.length} item${allBase.length === 1 ? "" : "s"} shown`),
      ),
      items.length > 0 && el(
        "div",
        { className: "review-header-nav" },
        el("button", { className: "icon-btn", onClick: () => setState({ groupIndex: (idx + items.length - 1) % items.length }) }, icon("chevronLeft", 13)),
        el("div", { className: "progress-text" }, `Item ${idx + 1} of ${items.length}`),
        el("button", { className: "icon-btn", onClick: () => setState({ groupIndex: (idx + 1) % items.length }) }, icon("chevronRight", 13)),
      ),
    ),
    state.actionMessage && el("div", { className: "hint" }, state.actionMessage),
    errorsPanel(),
    el(
      "div",
      { className: "review-3col" },
      fsPanel(allBase),
      dupTreePanel(items, idx, clearFilter),
      el(
        "div",
        { className: "review-main-panel" },
        items.length === 0
          ? el(
              "div",
              { className: "empty-state" },
              el("div", null, `No duplicates under ${state.fsSelectedPath}`),
              el("div", { className: "hint" }, "This folder wasn't part of a scan that found duplicates."),
              el("button", { className: "btn btn-ghost", onClick: clearFilter }, "Clear filter"),
            )
          : el(
              "div",
              { className: "breadcrumb" },
              icon("folder", 13),
              el("span", null, itemChain(current).concat(itemName(current)).join(" / ")),
            ),
        items.length > 0 && reviewMain(current),
      ),
    ),
  );
}

function viewHeader(title, subtitle) {
  return el(
    "div",
    { className: "view-header" },
    el("div", null, el("div", { className: "view-title" }, title), el("div", { className: "view-subtitle" }, subtitle)),
  );
}

// Panel 1: a real, lazily-expanded directory tree rooted at the scanned
// root (GUI-REVIEW-PANELS). Deliberately rooted there rather than the
// design handoff's whole-disk "/" mock -- every real duplicate is
// guaranteed to live under the scan root, so browsing the rest of the
// filesystem from here would have no filtering purpose, and would mean
// touching directories a scan never scanned (see `payload::DirEntryPayload`
// and ADR-0022's "no fabricated capability" precedent). Rows are colored
// by real scan status: accent + a count badge for a folder directly
// holding a duplicate, primary text for an ancestor of one, muted for
// everything else.
function fsPanel(allItems) {
  const collapsed = state.fsTreeCollapsed;
  if (collapsed) {
    return el(
      "div",
      { className: "fs-panel collapsed" },
      el("button", { className: "panel-toggle-btn", onClick: () => setState({ fsTreeCollapsed: false }), title: "Show file system panel" }, icon("chevronRight", 13)),
    );
  }
  if (!state.scanRoot) {
    return el(
      "div",
      { className: "fs-panel" },
      panelHeader("File system", true, () => setState({ fsTreeCollapsed: true })),
    );
  }

  ensureDirChildren(state.scanRoot, state.fsChildrenCache, () => render());

  const directCounts = new Map();
  for (const item of allItems) {
    for (const path of itemAllPaths(item)) {
      const dir = normPath(parentOf(path));
      if (!directCounts.has(dir)) directCounts.set(dir, new Set());
      directCounts.get(dir).add(item.key);
    }
  }
  const directKeys = [...directCounts.keys()];
  const hasDescendantDirect = (dir) => directKeys.some((k) => k !== dir && k.startsWith(dir + "/"));

  const rows = [];
  const walk = (path, depth) => {
    const children = state.fsChildrenCache[path];
    if (!children) return;
    for (const entry of children) {
      const norm = normPath(entry.path);
      const directCount = (directCounts.get(norm) || new Set()).size;
      const isAncestor = directCount === 0 && hasDescendantDirect(norm);
      const tier = directCount > 0 ? "direct" : isAncestor ? "ancestor" : "none";
      rows.push({ entry, depth, directCount, tier });
      if (state.fsOpenPaths.has(entry.path)) walk(entry.path, depth + 1);
    }
  };
  walk(state.scanRoot, 0);

  return el(
    "div",
    { className: "fs-panel" },
    panelHeader("File system", true, () => setState({ fsTreeCollapsed: true })),
    rows.length === 0
      ? el("div", { className: "empty-note" }, state.fsChildrenCache[state.scanRoot] === null ? "Loading…" : "No subfolders.")
      : rows.map((row) => fsRow(row)),
  );
}

function panelHeader(label, showLabel, onCollapse) {
  return el(
    "div",
    { style: "display:flex;align-items:center;gap:6px" },
    showLabel && el("div", { className: "panel-label", style: "flex:1" }, label),
    el("button", { className: "panel-toggle-btn", onClick: onCollapse, title: `Toggle ${label} panel` }, icon("chevronLeft", 13)),
  );
}

function fsRow({ entry, depth, directCount, tier }) {
  const expanded = state.fsOpenPaths.has(entry.path);
  const selected = state.fsSelectedPath === entry.path;
  const toggle = (e) => {
    e.stopPropagation();
    const open = new Set(state.fsOpenPaths);
    if (open.has(entry.path)) {
      open.delete(entry.path);
    } else {
      open.add(entry.path);
      ensureDirChildren(entry.path, state.fsChildrenCache, () => render());
    }
    setState({ fsOpenPaths: open });
  };
  const select = () =>
    setState(
      state.fsSelectedPath === entry.path
        ? { fsSelectedPath: null, groupIndex: 0 }
        : { fsSelectedPath: entry.path, groupIndex: 0 },
    );

  return el(
    "div",
    { className: "tree-row fs-row tier-" + tier + (selected ? " active" : ""), style: `padding-left:${8 + depth * 14}px`, onClick: select, title: entry.path },
    entry.hasChildren ? el("span", { className: "tree-chevron", onClick: toggle }, expanded ? "▾" : "▸") : el("span", { className: "tree-chevron" }),
    icon("folder", 13),
    el("span", { className: "tree-row-label" }, entry.name),
    directCount > 0 && el("span", { className: "fs-badge" }, String(directCount)),
  );
}

// Panel 2: a nested tree of the duplicate-group items themselves, grouped
// by real directory (e.g. "Documents › Finance") instead of a flat list --
// collapsible per folder section, same collapsed-state persistence as the
// design handoff's mockup.
function dupTreePanel(items, activeIdx, clearFilter) {
  if (state.dupTreeCollapsed) {
    return el(
      "div",
      { className: "dup-tree-panel collapsed" },
      el("button", { className: "panel-toggle-btn", onClick: () => setState({ dupTreeCollapsed: false }), title: "Show duplicate list panel" }, icon("chevronRight", 13)),
    );
  }

  const collapsedFolders = state.dupCollapsedFolders;
  const ancestorsCollapsed = (chain, upTo) => {
    for (let i = 1; i <= upTo; i++) if (collapsedFolders.has(chain.slice(0, i).join("/"))) return true;
    return false;
  };
  const rows = [];
  let prevChain = [];
  items.forEach((item, i) => {
    const chain = itemChain(item);
    let common = 0;
    while (common < prevChain.length && common < chain.length && prevChain[common] === chain[common]) common++;
    for (let d = common; d < chain.length; d++) {
      if (ancestorsCollapsed(chain, d)) continue;
      const key = chain.slice(0, d + 1).join("/");
      rows.push({ type: "folder", label: chain[d], key, depth: d, expanded: !collapsedFolders.has(key) });
    }
    if (!ancestorsCollapsed(chain, chain.length)) {
      rows.push({ type: "item", item, index: i, depth: chain.length });
    }
    prevChain = chain;
  });

  return el(
    "div",
    { className: "dup-tree-panel" },
    panelHeader("Duplicates", false, () => setState({ dupTreeCollapsed: true })),
    state.fsSelectedPath && el(
      "div",
      { className: "dup-filter-chip" },
      el("span", { style: "flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" }, `Filtered: ${state.fsSelectedPath}`),
      el("span", { style: "cursor:pointer;font-weight:700", onClick: clearFilter }, "✕"),
    ),
    ...rows.map((row) =>
      row.type === "folder"
        ? el(
            "div",
            {
              className: "dup-folder-header",
              style: `padding-left:${8 + row.depth * 16}px`,
              onClick: () => {
                const next = new Set(collapsedFolders);
                next.has(row.key) ? next.delete(row.key) : next.add(row.key);
                setState({ dupCollapsedFolders: next });
              },
            },
            el("span", { className: "tree-chevron" }, row.expanded ? "▾" : "▸"),
            icon("folder", 13),
            el("span", null, row.label),
          )
        : dupItemRow(row.item, row.index === activeIdx, row.depth, () => setState({ groupIndex: row.index })),
    ),
  );
}

function dupItemRow(item, active, depth, onClick) {
  const color = itemColor(item);
  return el(
    "div",
    { className: "tree-row group-row" + (active ? " active" : ""), style: `padding-left:${8 + depth * 16}px`, onClick },
    el("div", { className: "group-swatch", style: `background:${colorTint(color)};color:${color}` }, item.kind === "folder" ? icon("folder", 14) : (item.kind === "similar" ? icon("similar", 14) : null)),
    el(
      "div",
      { style: "flex:1;min-width:0" },
      el("div", { className: "group-row-name" }, itemName(item)),
      el("div", { className: "group-row-meta" }, itemMeta(item)),
    ),
  );
}

function reviewMain(item) {
  if (item.kind === "folder") return folderReviewMain(item);
  if (item.kind === "similar") return similarReviewMain(item);
  return fileReviewMain(item);
}

// Resolves (via the backend `choose_keep` command, SELECTION-RULES) which
// path a non-default keep rule would pick for `item`, caching the result in
// `state.ruleKeepChoice` so it's only looked up once per group. A manual
// keep-choice badge always wins over the rule, so this is skipped entirely
// once one exists. Also runs for the default "alphabetical" rule whenever
// protected folders are configured (ACTION-REFERENCE-FOLDERS) -- the
// guardrail overrides "alphabetically first" too, so the badge would
// otherwise show the wrong file as "kept" until Apply. Mutates `state`
// directly rather than via `setState` for the in-flight marker (`null`) to
// avoid re-rendering mid-render; the resolved result *does* go through
// `setState` once the lookup returns, so the card updates to reflect it.
function ensureRuleKeepChoice(item) {
  if (state.keepRule === "alphabetical" && referencePathsList().length === 0) return;
  if (state.keepChoice[item.key]) return;
  if (item.key in state.ruleKeepChoice) return;
  state.ruleKeepChoice[item.key] = null;
  const group = item.group;
  invoke("choose_keep", { group: { size: group.size, paths: group.paths }, rule: state.keepRule, referencePaths: referencePathsList() })
    .then((result) => {
      setState({ ruleKeepChoice: { ...state.ruleKeepChoice, [item.key]: result } });
    })
    .catch(() => {
      setState({
        ruleKeepChoice: {
          ...state.ruleKeepChoice,
          [item.key]: { keep: group.paths[0], reason: "alphabetically first (rule lookup failed)" },
        },
      });
    });
}

// Categories the backend's `read_preview` command can actually render
// (GUI-MEDIA-PREVIEW) -- video is deliberately excluded, see ADR-0028.
const PREVIEWABLE_CATEGORIES = new Set(["photo", "audio"]);

// Resolves (via the backend `read_preview` command) an inline thumbnail/
// player for `path`, caching the result in `state.previewCache` keyed by
// path -- shared across every group a given file happens to appear in,
// and looked up only once per path. Same in-flight-marker pattern as
// `ensureRuleKeepChoice`: mutates `state` directly for the `null` "in
// flight" marker to avoid a re-render mid-render, then goes through
// `setState` once the real result (a data: URI, or `false` for "not
// previewable") comes back.
function ensurePreview(path) {
  if (!PREVIEWABLE_CATEGORIES.has(categoryOf(path))) return;
  if (path in state.previewCache) return;
  state.previewCache[path] = null;
  invoke("read_preview", { path })
    .then((result) => {
      setState({ previewCache: { ...state.previewCache, [path]: result.dataUrl } });
    })
    .catch(() => {
      setState({ previewCache: { ...state.previewCache, [path]: false } });
    });
}

function fileReviewMain(item) {
  const group = item.group;
  ensureRuleKeepChoice(item);
  const ruleChoice = state.ruleKeepChoice[item.key];
  const manualChoice = state.keepChoice[item.key];
  const keepPath = manualChoice || (ruleChoice ? ruleChoice.keep : group.paths[0]);
  const keepReason = manualChoice ? "your choice" : ruleChoice ? ruleChoice.reason : "alphabetically first";
  const others = group.paths.filter((p) => p !== keepPath).length;

  const cards = group.paths.map((path, i) => {
    const keep = path === keepPath;
    const category = categoryOf(path);
    ensurePreview(path);
    const preview = state.previewCache[path];
    const hasPreview = typeof preview === "string";
    const thumb = hasPreview && category === "photo"
      ? el("img", { src: preview, alt: `preview of ${fileNameOf(path)}` })
      : icon("file", 26);
    return el(
      "div",
      { className: "compare-card " + (keep ? "keep" : "remove") },
      el("div", { className: "compare-thumb" }, thumb),
      el("div", { className: "compare-label" }, `Copy ${i + 1}`),
      el("div", { className: "compare-path" }, path),
      el(
        "div",
        { className: "compare-meta" },
        el("div", { className: "compare-meta-row" }, el("span", { className: "k" }, "Size"), el("span", null, bytesHuman(group.size))),
      ),
      hasPreview && category === "audio"
        ? el("audio", { src: preview, controls: true, className: "compare-audio" })
        : null,
      el(
        "button",
        { className: "compare-badge " + (keep ? "keep" : "remove"), onClick: () => setState({ keepChoice: { ...state.keepChoice, [item.key]: path } }) },
        keep && !manualChoice ? `Keeping this file — ${keepReason}` : keep ? "Keeping this file" : "Marked for removal",
      ),
    );
  });

  const actionVerb = { delete: "removed", trash: "trashed", hardlink: "hardlinked", reflink: "reflinked", move: "moved", copy: "copied" }[state.actionKind];
  const needsArchiveDir = ARCHIVE_KINDS.has(state.actionKind);
  const archiveDirMissing = needsArchiveDir && !state.archiveDir.trim();

  return el(
    "div",
    { className: "review-main" },
    el("div", { className: "compare-row" }, ...cards),
    el(
      "div",
      { className: "card review-action-bar" },
      el(
        "div",
        { className: "review-action-left" },
        el(
          "div",
          { className: "seg", style: "width:380px" },
          ...ACTION_KINDS.map((k) =>
            el(
              "button",
              { className: "seg-option" + (state.actionKind === k.id ? " active" : ""), onClick: () => setState({ actionKind: k.id }) },
              k.label,
            ),
          ),
        ),
        needsArchiveDir
          ? el(
              "div",
              { style: "margin-top:8px;max-width:380px" },
              pathInput("Archive folder (required) -- e.g. /home/me/duplicates-archive", state.archiveDir, (v) => { state.archiveDir = v; }),
            )
          : null,
        el(
          "div",
          { className: "reclaim-note" },
          state.actionKind === "copy"
            ? `${others} file${others === 1 ? "" : "s"} will be copied to the archive folder -- originals stay in place, nothing reclaimed`
            : [
                `${others} file${others === 1 ? "" : "s"} will be ${actionVerb} · reclaims `,
                el("span", { className: "reclaim-amount" }, bytesHuman(group.size * others)),
              ],
        ),
      ),
      el(
        "div",
        { className: "review-action-right" },
        el("button", { className: "btn btn-ghost", onClick: nextGroup }, "Skip"),
        el(
          "button",
          { className: "btn btn-danger", disabled: others === 0 || archiveDirMissing, onClick: () => applyAction(item, keepPath, keepReason) },
          `Apply ${ACTION_KINDS.find((k) => k.id === state.actionKind).label}`,
        ),
      ),
    ),
  );
}

// A perceptual "similar images" cluster (DETECTION-PERCEPTUAL-IMAGES,
// ADR-0030) -- deliberately read-only: no keep-choice, no action bar, no
// `run_action` call of any kind. "Similar" is not the byte-identical
// guarantee the action layer's destructive operations are built on, so
// this card only ever lets the user look and decide for themselves.
function similarReviewMain(item) {
  const group = item.group;

  const cards = group.paths.map((path) => {
    const category = categoryOf(path);
    ensurePreview(path);
    const preview = state.previewCache[path];
    const hasPreview = typeof preview === "string";
    const thumb = hasPreview && category === "photo"
      ? el("img", { src: preview, alt: `preview of ${fileNameOf(path)}` })
      : icon("file", 26);
    return el(
      "div",
      { className: "compare-card" },
      el("div", { className: "compare-thumb" }, thumb),
      el("div", { className: "compare-path" }, path),
    );
  });

  return el(
    "div",
    { className: "review-main" },
    el(
      "div",
      { className: "card", style: "border-color:var(--warning)" },
      el("div", { className: "card-title", style: "color:var(--warning)" }, "Similar images -- not confirmed identical"),
      el(
        "div",
        { className: "hint" },
        `These ${group.paths.length} images look visually similar (max difference ${group.maxDistance}/64 under a perceptual hash) but were not confirmed byte-identical by the exact scan -- review each one yourself before deleting anything.`,
      ),
    ),
    el("div", { className: "compare-row" }, ...cards),
    el(
      "div",
      { className: "card review-action-bar" },
      el("div", { className: "review-action-right" }, el("button", { className: "btn btn-ghost", onClick: nextGroup }, "Skip")),
    ),
  );
}

// Mirrors the CLI's `folder_match_pairs` (ADR-0023, `CLI-UX-001` FR-013):
// a `Contained` match always removes `subset` against `superset`; an
// `Exact` cluster of 2+ folders keeps one folder (the alphabetically-first
// one by default, matching `action::plan`'s "first path is kept"
// convention, or whichever one the user picked via the keep-choice badge)
// and removes every other folder against it.
function folderMatchPairs(item) {
  const match = item.match;
  if (match.type === "contained") {
    return [{ removed: match.subset, kept: match.superset }];
  }
  const sorted = [...match.folders].sort();
  const keptPath = state.keepChoice[item.key] || sorted[0];
  return match.folders
    .filter((f) => f !== keptPath)
    .map((f) => ({ removed: f, kept: keptPath }));
}

function folderReviewMain(item) {
  const match = item.match;
  const isExact = match.type === "exact";
  const paths = isExact ? match.folders : [match.subset, match.superset];
  const labels = isExact ? paths.map((_, i) => `Folder ${i + 1}`) : ["Subset", "Superset"];
  const pairs = folderMatchPairs(item);
  const keptPath = isExact ? pairs[0]?.kept ?? paths[0] : match.superset;

  const cards = paths.map((path, i) => {
    const keep = path === keptPath;
    return el(
      "div",
      { className: "compare-card " + (keep ? "keep" : "remove") },
      el("div", { className: "compare-thumb" }, icon("folder", 26)),
      el("div", { className: "compare-label" }, labels[i]),
      el("div", { className: "compare-path" }, path),
      el(
        "div",
        { className: "compare-meta" },
        el("div", { className: "compare-meta-row" }, el("span", { className: "k" }, "Size"), el("span", null, bytesHuman(match.bytes))),
        el("div", { className: "compare-meta-row" }, el("span", { className: "k" }, "Items"), el("span", null, `${match.fileCount} files`)),
      ),
      isExact
        ? el(
            "button",
            { className: "compare-badge " + (keep ? "keep" : "remove"), onClick: () => setState({ keepChoice: { ...state.keepChoice, [item.key]: path } }) },
            keep ? "Keeping this folder" : "Marked for removal",
          )
        : el("div", { className: "compare-badge " + (keep ? "keep" : "remove"), style: "cursor:default" }, keep ? "Superset (kept)" : "Subset (marked for removal)"),
    );
  });

  const removedCount = pairs.length;
  const totalFiles = removedCount * match.fileCount;
  const totalBytes = removedCount * match.bytes;
  const actionVerb = { delete: "removed", trash: "trashed", hardlink: "hardlinked", reflink: "reflinked", move: "moved", copy: "copied" }[state.actionKind];
  const actionLabel = ACTION_KINDS.find((k) => k.id === state.actionKind).label;
  const needsArchiveDir = ARCHIVE_KINDS.has(state.actionKind);
  const archiveDirMissing = needsArchiveDir && !state.archiveDir.trim();

  return el(
    "div",
    { className: "review-main" },
    el("div", { className: "compare-row" }, ...cards),
    el(
      "div",
      { className: "card review-action-bar" },
      el(
        "div",
        { className: "review-action-left" },
        el(
          "div",
          { className: "seg", style: "width:380px" },
          ...ACTION_KINDS.map((k) =>
            el(
              "button",
              { className: "seg-option" + (state.actionKind === k.id ? " active" : ""), onClick: () => setState({ actionKind: k.id }) },
              k.label,
            ),
          ),
        ),
        el("span", { className: "badge match-badge" }, isExact ? "Exact folder match" : "Contained folder match"),
        needsArchiveDir
          ? el(
              "div",
              { style: "margin-top:8px;max-width:380px" },
              pathInput("Archive folder (required) -- e.g. /home/me/duplicates-archive", state.archiveDir, (v) => { state.archiveDir = v; }),
            )
          : null,
        el(
          "div",
          { className: "reclaim-note" },
          state.actionKind === "copy"
            ? `${removedCount} folder${removedCount === 1 ? "" : "s"}, ${totalFiles} file${totalFiles === 1 ? "" : "s"} will be copied to the archive folder -- originals stay in place, nothing reclaimed`
            : [
                `${removedCount} folder${removedCount === 1 ? "" : "s"}, ${totalFiles} file${totalFiles === 1 ? "" : "s"} will be ${actionVerb} · reclaims `,
                el("span", { className: "reclaim-amount" }, bytesHuman(totalBytes)),
              ],
        ),
      ),
      el(
        "div",
        { className: "review-action-right" },
        el("button", { className: "btn btn-ghost", onClick: nextGroup }, "Skip"),
        el(
          "button",
          { className: "btn btn-danger", disabled: removedCount === 0 || archiveDirMissing, onClick: () => applyFolderAction(item) },
          `${actionLabel} Duplicate Folder${removedCount === 1 ? "" : "s"}`,
        ),
      ),
    ),
  );
}

function nextGroup() {
  const count = reviewItems().length;
  if (count === 0) return;
  setState({ groupIndex: (state.groupIndex + 1) % count });
}

async function applyAction(item, keepPath, keepReason) {
  const group = item.group;
  const ordered = [keepPath, ...group.paths.filter((p) => p !== keepPath)];
  const message = state.actionKind === "copy"
    ? `This will copy ${ordered.length - 1} file(s) to the archive folder. Originals stay in place -- nothing is reclaimed. Continue?`
    : `This will ${state.actionKind} ${ordered.length - 1} file(s), reclaiming ${bytesHuman(group.size * (ordered.length - 1))}. Continue?`;
  if (!window.confirm(message)) return;

  try {
    const result = await invoke("run_action", {
      group: { size: group.size, paths: ordered },
      kind: state.actionKind,
      keepReason,
      apply: true,
      referencePaths: referencePathsList(),
      archiveDir: state.archiveDir.trim() || null,
    });
    const reclaimed = result.applied ? result.applied.bytesReclaimed : 0;
    const failed = result.applied ? result.applied.failed.length : 0;
    setState({
      sessionBytesReclaimed: state.sessionBytesReclaimed + reclaimed,
      actionMessage: failed > 0
        ? `Reclaimed ${bytesHuman(reclaimed)}, ${failed} file(s) failed.`
        : `Reclaimed ${bytesHuman(reclaimed)}.`,
    });
  } catch (err) {
    setState({ actionMessage: `Error: ${err}` });
  }
  nextGroup();
}

async function applyFolderAction(item) {
  const pairs = folderMatchPairs(item);
  const totalFiles = pairs.length * item.match.fileCount;
  const totalBytes = pairs.length * item.match.bytes;
  const message = state.actionKind === "copy"
    ? `This will copy ${totalFiles} file(s) across ${pairs.length} folder${pairs.length === 1 ? "" : "s"} to the archive folder. Originals stay in place -- nothing is reclaimed. Continue?`
    : `This will ${state.actionKind} ${totalFiles} file(s) across ${pairs.length} folder${pairs.length === 1 ? "" : "s"}, reclaiming ${bytesHuman(totalBytes)}. Continue?`;
  if (!window.confirm(message)) return;

  let reclaimed = 0;
  let failed = 0;
  let error = null;
  for (const { removed, kept } of pairs) {
    try {
      const result = await invoke("run_folder_action", {
        removed,
        kept,
        groups: state.groups.map((g) => ({ size: g.size, paths: g.paths })),
        options: scanOptionsPayload(),
        kind: state.actionKind,
        apply: true,
        referencePaths: referencePathsList(),
        archiveDir: state.archiveDir.trim() || null,
      });
      reclaimed += result.applied ? result.applied.bytesReclaimed : 0;
      failed += result.applied ? result.applied.failed.length : 0;
    } catch (err) {
      error = String(err);
      break;
    }
  }

  setState({
    sessionBytesReclaimed: state.sessionBytesReclaimed + reclaimed,
    actionMessage: error
      ? `Reclaimed ${bytesHuman(reclaimed)} before an error: ${error}`
      : failed > 0
        ? `Reclaimed ${bytesHuman(reclaimed)}, ${failed} file(s) failed.`
        : `Reclaimed ${bytesHuman(reclaimed)}.`,
  });
  nextGroup();
}

// ---- rules (local-only preview -- ADR-0022; no backend exists yet) --------

function rulesView() {
  // "Keep newest copy" (rule id 1) is real (SELECTION-RULES) -- its on/off
  // state lives in `state.keepRule`, not `state.rules[].enabled`, and its
  // toggle drives every group in Review via the backend `choose_keep`
  // command. The other two rules are still an explicit local-only preview
  // (ADR-0022) -- unaffected by this.
  const keepNewestEnabled = state.keepRule === "newest";

  return el(
    "div",
    { className: "view" },
    el(
      "div",
      { className: "view-header" },
      el(
        "div",
        null,
        el("div", { className: "view-title" }, "Rules & Automation"),
        el(
          "div",
          { className: "view-subtitle" },
          "\"Keep newest copy\" applies live in Review. Everything else here is preview only.",
        ),
      ),
    ),
    el(
      "div",
      { className: "rule-list" },
      ...state.rules.map((r) => {
        const isKeepNewest = r.id === 1;
        const enabled = isKeepNewest ? keepNewestEnabled : r.enabled;
        const onClick = isKeepNewest
          ? () =>
              setState({
                keepRule: keepNewestEnabled ? "alphabetical" : "newest",
                ruleKeepChoice: {},
              })
          : () => setState({ rules: state.rules.map((x) => (x.id === r.id ? { ...x, enabled: !x.enabled } : x)) });
        return el(
          "div",
          { className: "card rule-card" },
          el("div", { className: "rule-icon" + (enabled ? " enabled" : "") }, icon("shield", 17)),
          el("div", { className: "rule-body" }, el("div", { className: "rule-title" }, r.title), el("div", { className: "rule-desc" }, r.desc)),
          el(
            "button",
            { className: "toggle-track" + (enabled ? " on" : ""), onClick },
            el("div", { className: "toggle-knob" }),
          ),
        );
      }),
    ),
    el(
      "div",
      { className: "empty-note", style: "text-align:center" },
      "\"Keep newest copy\" is applied for real in Review. \"Ignore tiny files\" and \"Auto-clean Downloads\" aren't wired to real scans yet -- local preview only.",
    ),
  );
}

// ---- scan orchestration ---------------------------------------------------

async function startScan() {
  if (!state.scanRoot.trim()) {
    setState({ scanError: "Enter a directory to scan first." });
    return;
  }

  setState({
    scanning: true,
    scanError: null,
    actionMessage: null,
    groups: [],
    folderMatches: [],
    similarGroups: [],
    errors: [],
    progress: { filesScanned: 0, bytesScanned: 0 },
    groupIndex: 0,
    keepChoice: {},
    ruleKeepChoice: {},
    // A prior scan's file-system tree/filter is meaningless once the root
    // changes -- start every new scan with a fresh Review panel state.
    fsChildrenCache: {},
    fsOpenPaths: new Set(),
    fsSelectedPath: null,
    dupCollapsedFolders: new Set(),
    view: "review",
  });

  try {
    await invoke("start_scan", { root: state.scanRoot, options: scanOptionsPayload() });
  } catch (err) {
    setState({ scanning: false, scanError: String(err), view: "scan" });
  }
}

// Parses a comma-separated field into a trimmed, non-empty-entry array, or
// `null` if the field is blank (meaning "no filter" -- see `ScanOptions`'s
// own `None` default for include/exclude extension lists).
function parseCsv(value) {
  if (!value || !value.trim()) return null;
  const items = value.split(",").map((s) => s.trim()).filter(Boolean);
  return items.length ? items : null;
}

// Parses a size field (bytes) into a non-negative integer, or `null` if
// blank or not a valid number.
function parseSize(value) {
  if (!value || !value.trim()) return null;
  const n = Number(value.trim());
  return Number.isFinite(n) && n >= 0 ? Math.floor(n) : null;
}

// Parses `state.referencePaths` into the list `run_action`/`choose_keep`/
// `run_folder_action` expect -- unlike `scanOptionsPayload`'s CSV fields,
// this is never `null`; the backend's `referencePaths` param is a required
// (possibly empty) array, not an `Option`.
function referencePathsList() {
  return parseCsv(state.referencePaths) || [];
}

function scanOptionsPayload() {
  const o = state.options;
  return {
    followSymlinks: o.followSymlinks,
    crossFilesystems: o.crossFilesystems,
    verifyMatches: o.verifyMatches,
    ioThreads: o.ioThreads,
    cachePath: o.cachePath || null,
    fclonesImportPath: o.fclonesImportPath || null,
    minSize: parseSize(o.minSize),
    maxSize: parseSize(o.maxSize),
    includeExtensions: parseCsv(o.includeExtensions),
    excludeExtensions: parseCsv(o.excludeExtensions),
    excludePaths: parseCsv(o.excludePaths) || [],
  };
}

// ---- scan profiles (SCAN-PROFILES, ADR-0029) ------------------------------

// Fetched once at startup (see the bottom of this file) and refreshed from
// each save/delete response -- never re-fetched per render.
async function loadScanProfiles() {
  try {
    const profiles = await invoke("list_scan_profiles");
    setState({ scanProfiles: profiles });
  } catch (err) {
    // Best-effort: if the OS config directory can't be resolved, saved
    // profiles just won't be listed this session -- not worth blocking the
    // rest of the app over.
  }
}

async function saveScanProfile() {
  const name = state.profileNameInput.trim();
  if (!name) {
    setState({ profileError: "Enter a name for this profile first." });
    return;
  }
  try {
    const profiles = await invoke("save_scan_profile", {
      name,
      root: state.scanRoot,
      options: scanOptionsPayload(),
    });
    setState({ scanProfiles: profiles, profileNameInput: "", profileError: null });
  } catch (err) {
    setState({ profileError: String(err) });
  }
}

// Reverse of `scanOptionsPayload` -- turns a saved profile's parsed
// options back into `state.options`'s string-based form fields (numbers
// and arrays become the same comma-joined/plain-text strings the Scan
// Setup inputs display and re-parse on save).
function applyScanProfile(profile) {
  const o = profile.options || {};
  setState({
    scanRoot: profile.root,
    profileError: null,
    options: {
      followSymlinks: !!o.followSymlinks,
      crossFilesystems: !!o.crossFilesystems,
      verifyMatches: !!o.verifyMatches,
      ioThreads: o.ioThreads ?? null,
      cachePath: o.cachePath || "",
      fclonesImportPath: o.fclonesImportPath || "",
      minSize: o.minSize != null ? String(o.minSize) : "",
      maxSize: o.maxSize != null ? String(o.maxSize) : "",
      includeExtensions: (o.includeExtensions || []).join(", "),
      excludeExtensions: (o.excludeExtensions || []).join(", "),
      excludePaths: (o.excludePaths || []).join(", "),
    },
  });
}

async function deleteScanProfile(name) {
  try {
    const profiles = await invoke("delete_scan_profile", { name });
    setState({ scanProfiles: profiles });
  } catch (err) {
    setState({ profileError: String(err) });
  }
}

listen("scan-event", (event) => {
  const payload = event.payload;
  switch (payload.type) {
    case "duplicate_group":
      setState({ groups: [...state.groups, { size: payload.size, paths: payload.paths }] });
      break;
    case "progress":
      setState({ progress: { filesScanned: payload.filesScanned, bytesScanned: payload.bytesScanned } });
      break;
    case "error":
      setState({ errors: [...state.errors, payload] });
      break;
    case "finished":
      onScanFinished(payload);
      break;
    default:
      break;
  }
});

// Dashboard's "Export (JSON)" button (`CLI-HISTORY-AUDIT`): downloads this
// session's in-memory `state.scanHistory` (already tracked for the Recent
// Scans table) as a JSON file, via the standard `<a download>` + object-URL
// technique -- a real webview download, not a native save dialog, so it
// needed no new Tauri plugin/permission. Session-scoped only, the same
// caveat the Recent Scans table already carries: the CLI's `--history`
// (persisted SQLite, survives across launches) is the durable option.
function exportScanHistoryJson() {
  const json = JSON.stringify(state.scanHistory, null, 2);
  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `rusty-fclone-scan-history-${Date.now()}.json`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

async function onScanFinished(summary) {
  const reclaimEstimate = state.groups.reduce((sum, g) => sum + g.size * Math.max(g.paths.length - 1, 0), 0);
  const record = {
    root: state.scanRoot,
    finishedAt: Date.now(),
    duplicateFiles: summary.duplicateFiles,
    reclaimEstimate,
  };
  setState({
    scanning: false,
    lastSummary: { ...summary, finishedAt: record.finishedAt },
    scanHistory: [record, ...state.scanHistory],
  });

  if (state.groups.length > 0) {
    setState({ findingFolders: true });
    try {
      const matches = await invoke("find_duplicate_folders", {
        root: state.scanRoot,
        groups: state.groups.map((g) => ({ size: g.size, paths: g.paths })),
        options: scanOptionsPayload(),
      });
      setState({ folderMatches: matches, findingFolders: false });
    } catch (err) {
      // A folder-dedup failure (e.g. the root vanished between the scan
      // and this follow-up call) shouldn't hide the file-level results
      // already shown -- just stop looking for folder matches this round.
      setState({ findingFolders: false });
    }
  }

  // Independent of `state.groups.length` -- unlike folder-dedup, a
  // perceptual cluster doesn't depend on any exact `DuplicateGroup`
  // existing at all (DETECTION-PERCEPTUAL-IMAGES, ADR-0030).
  if (state.matchMode === "similar") {
    setState({ findingSimilarImages: true });
    try {
      const groups = await invoke("find_similar_images", {
        root: state.scanRoot,
        options: scanOptionsPayload(),
      });
      setState({ similarGroups: groups, findingSimilarImages: false });
    } catch (err) {
      setState({ findingSimilarImages: false });
    }
  }
}

render();
loadScanProfiles();
