import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./taskbar.css";

interface TaskbarTrafficView {
  download: string;
  upload: string;
}

const down = document.getElementById("down");
const up = document.getElementById("up");

function log(...args: unknown[]): void {
  console.log("[taskbar]", ...args);
}

function native(): boolean {
  return (
    typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined
  );
}

function render(view: TaskbarTrafficView): void {
  if (down) down.textContent = view.download;
  if (up) up.textContent = view.upload;
}

async function showMain(): Promise<void> {
  if (!native()) return;
  await invoke("show_main_window");
}

if (native()) {
  log("overlay mounted; subscribing to taskbar-traffic events");
  listen<TaskbarTrafficView>("taskbar-traffic", (event) => {
    render(event.payload);
  }).catch((cause) => {
    log("listen failed:", cause);
  });
} else {
  log("not running inside Tauri; overlay stays static");
}

document.addEventListener("click", () => {
  void showMain();
});

document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});
