import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

const zipList = document.getElementById("zip-list") as HTMLUListElement;
const zipEmpty = document.getElementById("zip-empty") as HTMLElement;
const dropZone = document.getElementById("drop-zone") as HTMLElement;
const addZipsBtn = document.getElementById("add-zips") as HTMLButtonElement;
const outputDirInput = document.getElementById("output-dir") as HTMLInputElement;
const selectOutputBtn = document.getElementById("select-output") as HTMLButtonElement;
const divideDatesCheck = document.getElementById("divide-dates") as HTMLInputElement;
const skipExtrasCheck = document.getElementById("skip-extras") as HTMLInputElement;
const noGuessCheck = document.getElementById("no-guess") as HTMLInputElement;
const runBtn = document.getElementById("run-btn") as HTMLButtonElement;
const progressSection = document.getElementById("progress-section") as HTMLElement;
const progressStage = document.getElementById("progress-stage") as HTMLElement;
const progressPct = document.getElementById("progress-pct") as HTMLElement;
const progressFill = document.getElementById("progress-fill") as HTMLElement;
const progressDetail = document.getElementById("progress-detail") as HTMLElement;
const resultSection = document.getElementById("result-section") as HTMLElement;
const resultText = document.getElementById("result-text") as HTMLParagraphElement;
const logOutput = document.getElementById("log-output") as HTMLPreElement;

let zipFiles: string[] = [];

function addZipPaths(paths: string[]) {
  for (const p of paths) {
    if (p.toLowerCase().endsWith(".zip") && !zipFiles.includes(p)) {
      zipFiles.push(p);
    }
  }
  renderZipList();
}

function renderZipList() {
  zipList.innerHTML = "";
  zipEmpty.hidden = zipFiles.length > 0;

  for (const file of zipFiles) {
    const li = document.createElement("li");

    const nameSpan = document.createElement("span");
    nameSpan.className = "filename";
    nameSpan.textContent = file.split(/[\\/]/).pop() || file;
    nameSpan.title = file;

    const removeBtn = document.createElement("button");
    removeBtn.className = "remove-btn";
    removeBtn.textContent = "\u00d7";
    removeBtn.title = "Remove";
    removeBtn.onclick = (e) => {
      e.stopPropagation();
      zipFiles = zipFiles.filter((f) => f !== file);
      renderZipList();
    };

    li.appendChild(nameSpan);
    li.appendChild(removeBtn);
    zipList.appendChild(li);
  }
}

function log(msg: string) {
  logOutput.textContent += msg + "\n";
  logOutput.scrollTop = logOutput.scrollHeight;
}

// Drag & drop via Tauri events
listen<{ paths: string[]; position: { x: number; y: number } }>(
  "tauri://drag-enter",
  () => { dropZone.classList.add("drag-over"); }
);

listen("tauri://drag-leave", () => {
  dropZone.classList.remove("drag-over");
});

listen<{ paths: string[]; position: { x: number; y: number } }>(
  "tauri://drag-drop",
  (event) => {
    dropZone.classList.remove("drag-over");
    if (event.payload.paths) {
      addZipPaths(event.payload.paths);
    }
  }
);

// File dialog
addZipsBtn.onclick = async () => {
  try {
    const selected = await open({
      multiple: true,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (selected) {
      addZipPaths(Array.isArray(selected) ? selected : [selected]);
    }
  } catch (e) {
    log("Error selecting files: " + e);
  }
};

selectOutputBtn.onclick = async () => {
  try {
    const dir = await open({ directory: true });
    if (dir) {
      outputDirInput.value = dir;
    }
  } catch (e) {
    log("Error selecting directory: " + e);
  }
};

// Progress
const stageLabels: Record<string, string> = {
  scan: "Scanning ZIP files",
  date: "Extracting dates",
  "date-exif": "Reading EXIF data",
  dedup: "Deduplicating",
  write: "Writing files",
};

listen<{ stage: string; current: number; total: number; message: string }>(
  "progress",
  (event) => {
    const p = event.payload;
    progressSection.hidden = false;
    progressStage.textContent = stageLabels[p.stage] || p.stage;
    const pct = p.total > 0 ? Math.round((p.current / p.total) * 100) : 0;
    progressPct.textContent = pct + "%";
    progressFill.style.width = pct + "%";
    progressDetail.textContent = `${p.current.toLocaleString()} / ${p.total.toLocaleString()}`;
  }
);

// Run
runBtn.onclick = async () => {
  if (zipFiles.length === 0) {
    log("Error: No ZIP files selected");
    return;
  }
  if (!outputDirInput.value) {
    log("Error: No output directory selected");
    return;
  }

  runBtn.disabled = true;
  runBtn.textContent = "Processing...";
  resultSection.hidden = true;
  progressSection.hidden = false;
  progressFill.style.width = "0%";
  progressFill.style.background = "";
  progressPct.textContent = "0%";
  progressStage.textContent = "Starting...";
  progressDetail.textContent = "";
  logOutput.textContent = "";

  try {
    const result = await invoke<string>("run_process", {
      options: {
        zip_files: zipFiles,
        output: outputDirInput.value,
        divide_to_dates: divideDatesCheck.checked,
        skip_extras: skipExtrasCheck.checked,
        no_guess: noGuessCheck.checked,
      },
    });
    progressSection.hidden = true;
    resultSection.hidden = false;
    resultText.textContent = result;
    log("Done: " + result);
  } catch (e) {
    log("Error: " + e);
    progressStage.textContent = "Error";
    progressFill.style.width = "100%";
    progressFill.style.background = "var(--red)";
  } finally {
    runBtn.disabled = false;
    runBtn.textContent = "Run";
  }
};
