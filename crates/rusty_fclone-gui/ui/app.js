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
];

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
  scanning: false,
  scanError: null,
  progress: { filesScanned: 0, bytesScanned: 0 },
  groups: [],
  folderMatches: [],
  findingFolders: false,
  lastSummary: null,
  scanHistory: [],
  errors: [],
  groupIndex: 0,
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
  sessionBytesReclaimed: 0,
  actionMessage: null,
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
    else if (key === "onClick") node.addEventListener("click", value);
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

function render() {
  document.documentElement.setAttribute("data-theme", state.theme);
  app.replaceChildren(sidebar(), content());
}

function sidebar() {
  const navItem = (id, label, view) =>
    el(
      "button",
      { className: "nav-item" + (state.view === view ? " active" : ""), onClick: () => setState({ view }) },
      icon(id, 17),
      el("span", null, label),
    );

  return el(
    "div",
    { className: "sidebar" },
    el(
      "div",
      null,
      el(
        "div",
        { className: "brand" },
        el("div", { className: "brand-icon" }, icon("logo", 15)),
        el("div", { className: "brand-name" }, "Rusty FClone"),
      ),
      el(
        "div",
        { className: "nav" },
        navItem("dashboard", "Dashboard", "dashboard"),
        navItem("scan", "Scan", "scan"),
        navItem("review", "Review", "review"),
        navItem("rules", "Rules", "rules"),
      ),
    ),
    el(
      "div",
      null,
      el("div", { className: "sidebar-footer-divider" }),
      el(
        "button",
        { className: "theme-toggle", onClick: toggleTheme },
        icon(state.theme === "dark" ? "sun" : "moon", 15),
        el("span", null, state.theme === "dark" ? "Light mode" : "Dark mode"),
      ),
      el(
        "div",
        { className: "session-savings" },
        el("div", { className: "session-savings-label" }, "Reclaimed this session"),
        el("div", { className: "session-savings-value" }, bytesHuman(state.sessionBytesReclaimed)),
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
        ...breakdown.map((b) => el("div", { style: `width:${b.pct}%;background:${b.color}` })),
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
                `${b.label} ${b.pct}%`,
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
          { className: "btn btn-ghost", disabled: true, title: "Reading saved scan history isn't wired into the GUI yet -- see the CLI's --history flag" },
          icon("dashboard", 13),
          "Import history",
        ),
        el(
          "button",
          { className: "btn btn-ghost", disabled: true, title: "Exporting to a file needs a native save dialog, not wired into the GUI yet" },
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
    { className: "view" },
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
        pathInput("e.g. /home/me/Pictures", state.scanRoot, (v) => { state.scanRoot = v; }),
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
            el("button", { className: "seg-option active" }, "Exact match"),
            el("button", { className: "seg-option", disabled: true, title: "Fuzzy/near-duplicate matching isn't implemented yet" }, "Similar content"),
          ),
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

function reviewItems() {
  const files = state.groups
    .filter((g) => state.typeFilter.has(categoryOf(g.paths[0])))
    .map((g, i) => ({ kind: "file", group: g, key: `file-${i}` }));
  const folders = state.folderMatches.map((m, i) => ({ kind: "folder", match: m, key: `folder-${i}` }));
  return files.concat(folders);
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
  const items = reviewItems();
  if (items.length === 0) {
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

  const idx = Math.min(state.groupIndex, items.length - 1);
  const current = items[idx];

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
        el("div", { className: "view-subtitle" }, state.scanning ? scanningSubtitle() : `${items.length} item${items.length === 1 ? "" : "s"} to review`),
      ),
      el(
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
      { className: "review-layout" },
      el(
        "div",
        { className: "group-list" },
        ...items.map((item, i) => groupListRow(item, i === idx, () => setState({ groupIndex: i }))),
      ),
      reviewMain(current),
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

function groupListRow(item, active, onClick) {
  const isFolder = item.kind === "folder";
  const color = isFolder ? "var(--pink)" : KIND_COLOR[categoryOf(item.group.paths[0])] || "var(--accent)";
  const name = isFolder
    ? fileNameOf(item.match.type === "exact" ? item.match.folders[0] : item.match.subset)
    : fileNameOf(item.group.paths[0]);
  const meta = isFolder
    ? `${item.match.fileCount} files · ${bytesHuman(item.match.bytes)}`
    : `${item.group.paths.length} copies · ${bytesHuman(item.group.size)}`;

  return el(
    "button",
    { className: "group-row" + (active ? " active" : ""), onClick },
    el("div", { className: "group-swatch", style: `background:${colorTint(color)};color:${color}` }, isFolder ? icon("folder", 15) : null),
    el(
      "div",
      { style: "flex:1;min-width:0" },
      el("div", { className: "group-row-name" }, name),
      el("div", { className: "group-row-meta" }, meta),
    ),
  );
}

function colorTint(cssVar) {
  // Every KIND_COLOR entry is a var(--token) reference; the *-tint custom
  // properties already exist for accent/success/danger/pink, but per-kind
  // tints (purple/warning/other) don't have a dedicated variable, so tint
  // generically via color-mix, which every target webview (WebKitGTK,
  // WebView2, WKWebView) supports.
  return `color-mix(in srgb, ${cssVar} 16%, transparent)`;
}

function reviewMain(item) {
  if (item.kind === "folder") return folderReviewMain(item);
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
    return el(
      "div",
      { className: "compare-card " + (keep ? "keep" : "remove") },
      el("div", { className: "compare-thumb" }, icon("file", 26)),
      el("div", { className: "compare-label" }, `Copy ${i + 1}`),
      el("div", { className: "compare-path" }, path),
      el(
        "div",
        { className: "compare-meta" },
        el("div", { className: "compare-meta-row" }, el("span", { className: "k" }, "Size"), el("span", null, bytesHuman(group.size))),
      ),
      el(
        "button",
        { className: "compare-badge " + (keep ? "keep" : "remove"), onClick: () => setState({ keepChoice: { ...state.keepChoice, [item.key]: path } }) },
        keep && !manualChoice ? `Keeping this file — ${keepReason}` : keep ? "Keeping this file" : "Marked for removal",
      ),
    );
  });

  const actionVerb = { delete: "removed", trash: "trashed", hardlink: "hardlinked", reflink: "reflinked" }[state.actionKind];

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
          { className: "seg", style: "width:260px" },
          ...ACTION_KINDS.map((k) =>
            el(
              "button",
              { className: "seg-option" + (state.actionKind === k.id ? " active" : ""), onClick: () => setState({ actionKind: k.id }) },
              k.label,
            ),
          ),
        ),
        el(
          "div",
          { className: "reclaim-note" },
          `${others} file${others === 1 ? "" : "s"} will be ${actionVerb} · reclaims `,
          el("span", { className: "reclaim-amount" }, bytesHuman(group.size * others)),
        ),
      ),
      el(
        "div",
        { className: "review-action-right" },
        el("button", { className: "btn btn-ghost", onClick: nextGroup }, "Skip"),
        el(
          "button",
          { className: "btn btn-danger", disabled: others === 0, onClick: () => applyAction(item, keepPath, keepReason) },
          `Apply ${ACTION_KINDS.find((k) => k.id === state.actionKind).label}`,
        ),
      ),
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
  const actionVerb = { delete: "removed", trash: "trashed", hardlink: "hardlinked", reflink: "reflinked" }[state.actionKind];
  const actionLabel = ACTION_KINDS.find((k) => k.id === state.actionKind).label;

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
          { className: "seg", style: "width:260px" },
          ...ACTION_KINDS.map((k) =>
            el(
              "button",
              { className: "seg-option" + (state.actionKind === k.id ? " active" : ""), onClick: () => setState({ actionKind: k.id }) },
              k.label,
            ),
          ),
        ),
        el("span", { className: "badge match-badge" }, isExact ? "Exact folder match" : "Contained folder match"),
        el(
          "div",
          { className: "reclaim-note" },
          `${removedCount} folder${removedCount === 1 ? "" : "s"}, ${totalFiles} file${totalFiles === 1 ? "" : "s"} will be ${actionVerb} · reclaims `,
          el("span", { className: "reclaim-amount" }, bytesHuman(totalBytes)),
        ),
      ),
      el(
        "div",
        { className: "review-action-right" },
        el("button", { className: "btn btn-ghost", onClick: nextGroup }, "Skip"),
        el(
          "button",
          { className: "btn btn-danger", disabled: removedCount === 0, onClick: () => applyFolderAction(item) },
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
  const message = `This will ${state.actionKind} ${ordered.length - 1} file(s), reclaiming ${bytesHuman(group.size * (ordered.length - 1))}. Continue?`;
  if (!window.confirm(message)) return;

  try {
    const result = await invoke("run_action", {
      group: { size: group.size, paths: ordered },
      kind: state.actionKind,
      keepReason,
      apply: true,
      referencePaths: referencePathsList(),
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
  const message = `This will ${state.actionKind} ${totalFiles} file(s) across ${pairs.length} folder${pairs.length === 1 ? "" : "s"}, reclaiming ${bytesHuman(totalBytes)}. Continue?`;
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
    errors: [],
    progress: { filesScanned: 0, bytesScanned: 0 },
    groupIndex: 0,
    keepChoice: {},
    ruleKeepChoice: {},
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

  if (state.groups.length === 0) return;
  setState({ findingFolders: true });
  try {
    const matches = await invoke("find_duplicate_folders", {
      root: state.scanRoot,
      groups: state.groups.map((g) => ({ size: g.size, paths: g.paths })),
      options: scanOptionsPayload(),
    });
    setState({ folderMatches: matches, findingFolders: false });
  } catch (err) {
    // A folder-dedup failure (e.g. the root vanished between the scan and
    // this follow-up call) shouldn't hide the file-level results already
    // shown -- just stop looking for folder matches this round.
    setState({ findingFolders: false });
  }
}

render();
