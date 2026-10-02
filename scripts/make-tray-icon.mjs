// Renders the menu-bar template icon from its SVG source using the Tauri CLI.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const out = mkdtempSync(join(tmpdir(), "tf-tray-"));
try {
  execFileSync("npx", ["tauri", "icon", "src-tauri/icons/source/tray-icon.svg", "-o", out, "-p", "44"], {
    stdio: "inherit",
  });
  copyFileSync(join(out, "44x44.png"), "src-tauri/icons/tray-icon.png");
  console.warn("Wrote src-tauri/icons/tray-icon.png");
} finally {
  rmSync(out, { recursive: true, force: true });
}
