// Vanilla JS frontend -- no bundler, no npm dependency. Talks to the Rust
// backend (crates/rusty_fclone-gui/src/commands.rs) over Tauri's IPC via
// the `withGlobalTauri` bridge (see tauri.conf.json).
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const rootInput = document.getElementById("root-path");
const scanBtn = document.getElementById("scan-btn");
const statusEl = document.getElementById("status");
const groupsEl = document.getElementById("groups");
const errorsEl = document.getElementById("errors");

const ACTION_KINDS = ["delete", "hardlink", "reflink"];

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

function readOptions() {
  const num = (id) => {
    const v = document.getElementById(id).value.trim();
    return v === "" ? null : Number(v);
  };
  const str = (id) => {
    const v = document.getElementById(id).value.trim();
    return v === "" ? null : v;
  };
  return {
    followSymlinks: document.getElementById("opt-follow-symlinks").checked,
    crossFilesystems: document.getElementById("opt-cross-filesystems").checked,
    verifyMatches: document.getElementById("opt-verify-matches").checked,
    smallFileThreshold: num("opt-small-file-threshold"),
    partialHashSampleSize: num("opt-partial-hash-sample-size"),
    ioThreads: num("opt-io-threads"),
    cachePath: str("opt-cache-path"),
    fclonesImportPath: str("opt-fclones-import-path"),
  };
}

function groupCard(group) {
  const card = document.createElement("div");
  card.className = "group";

  const header = document.createElement("div");
  header.className = "group-header";
  header.innerHTML = `<span>${bytesHuman(group.size)} &times; ${group.paths.length} copies</span>`;
  card.appendChild(header);

  const list = document.createElement("ul");
  for (const path of group.paths) {
    const li = document.createElement("li");
    li.textContent = path;
    list.appendChild(li);
  }
  card.appendChild(list);

  const actions = document.createElement("div");
  actions.className = "group-actions";

  const select = document.createElement("select");
  for (const kind of ACTION_KINDS) {
    const opt = document.createElement("option");
    opt.value = kind;
    opt.textContent = kind;
    select.appendChild(opt);
  }
  actions.appendChild(select);

  const applyLabel = document.createElement("label");
  const applyCheckbox = document.createElement("input");
  applyCheckbox.type = "checkbox";
  applyLabel.appendChild(applyCheckbox);
  applyLabel.appendChild(document.createTextNode(" Apply (uncheck to preview)"));
  actions.appendChild(applyLabel);

  const runBtn = document.createElement("button");
  runBtn.className = "secondary";
  runBtn.textContent = "Run";
  actions.appendChild(runBtn);

  card.appendChild(actions);

  const resultEl = document.createElement("div");
  resultEl.className = "group-result";
  card.appendChild(resultEl);

  runBtn.addEventListener("click", async () => {
    runBtn.disabled = true;
    resultEl.classList.remove("error");
    resultEl.textContent = "Running...";
    try {
      const result = await invoke("run_action", {
        group: { size: group.size, paths: group.paths },
        kind: select.value,
        apply: applyCheckbox.checked,
      });
      resultEl.textContent = describeActionResult(result, applyCheckbox.checked);
    } catch (err) {
      resultEl.classList.add("error");
      resultEl.textContent = `Error: ${err}`;
    } finally {
      runBtn.disabled = false;
    }
  });

  return card;
}

function describeActionResult(result, applied) {
  const { plan, applied: report } = result;
  if (!applied || !report) {
    return `Preview: keep ${plan.kept}; would act on ${plan.planned.length} file(s), reclaiming ${bytesHuman(plan.bytesReclaimed)}.`;
  }
  const failed = report.failed.length > 0 ? `, ${report.failed.length} failed` : "";
  return `Kept ${plan.kept}; ${report.succeeded.length} file(s) done${failed}, ${bytesHuman(report.bytesReclaimed)} reclaimed.`;
}

async function startScan() {
  const root = rootInput.value.trim();
  if (!root) {
    statusEl.textContent = "Enter a directory to scan first.";
    return;
  }
  groupsEl.innerHTML = "";
  errorsEl.innerHTML = "";
  scanBtn.disabled = true;
  statusEl.textContent = "Scanning...";

  try {
    await invoke("start_scan", { root, options: readOptions() });
  } catch (err) {
    statusEl.textContent = `Failed to start scan: ${err}`;
    scanBtn.disabled = false;
  }
}

listen("scan-event", (event) => {
  const payload = event.payload;
  switch (payload.type) {
    case "duplicate_group":
      groupsEl.appendChild(groupCard(payload));
      break;
    case "progress":
      statusEl.textContent = `Scanning... ${payload.filesScanned} files, ${bytesHuman(payload.bytesScanned)} scanned so far.`;
      break;
    case "error": {
      const line = document.createElement("div");
      line.textContent = `${payload.path}: ${payload.message}`;
      errorsEl.appendChild(line);
      break;
    }
    case "finished":
      statusEl.textContent = `Done: ${payload.filesScanned} files scanned, ${payload.duplicateGroups} duplicate group(s) (${payload.duplicateFiles} redundant files).`;
      scanBtn.disabled = false;
      break;
    default:
      break;
  }
});

scanBtn.addEventListener("click", startScan);
