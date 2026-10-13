import { isTauri } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type AppLayout = "expanded" | "collapsed";

const storageKey = "lobbylocker.layout";
const minimum = {
  expanded: new LogicalSize(760, 560),
  collapsed: new LogicalSize(360, 560),
};

export function loadLayout(): AppLayout {
  try {
    return localStorage.getItem(storageKey) === "collapsed"
      ? "collapsed"
      : "expanded";
  } catch {
    return "expanded";
  }
}

export function saveLayout(layout: AppLayout) {
  try {
    localStorage.setItem(storageKey, layout);
  } catch {
    // Layout switching still works when webview storage is unavailable.
  }
}

/** Window-only changes: never touch game selections or firewall preferences. */
export class WindowLayout {
  private current: AppLayout = "expanded";
  private sizes = {
    expanded: new LogicalSize(1240, 860),
    collapsed: new LogicalSize(420, 800),
  };

  async set(next: AppLayout): Promise<void> {
    if (!isTauri() || next === this.current) return;
    const window = getCurrentWindow();
    const [physical, scale] = await Promise.all([
      window.innerSize(),
      window.scaleFactor(),
    ]);
    const previousSize = physical.toLogical(scale);
    // Lower the native minimum before requesting the narrow window. Maximum
    // sizes and maximization settings from tauri.conf.json stay untouched.
    await window.setMinSize(minimum[next]);
    try {
      await window.setSize(this.sizes[next]);
    } catch (error) {
      try {
        await window.setMinSize(minimum[this.current]);
        await window.setSize(previousSize);
      } catch {
        // Keep the original resize failure rather than a secondary rollback error.
      }
      throw error;
    }
    this.sizes[this.current] = previousSize;
    this.current = next;
  }
}
