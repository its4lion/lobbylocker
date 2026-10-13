import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LogicalSize, PhysicalSize } from "@tauri-apps/api/dpi";
import { loadLayout, saveLayout, WindowLayout } from "./layout";

const native = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  getCurrentWindow: vi.fn(),
  window: {
    innerSize: vi.fn(),
    scaleFactor: vi.fn(),
    setMinSize: vi.fn(),
    setSize: vi.fn(),
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: native.isTauri }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: native.getCurrentWindow,
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.unstubAllGlobals();
  native.isTauri.mockReturnValue(true);
  native.getCurrentWindow.mockReturnValue(native.window);
  native.window.innerSize.mockResolvedValue(new PhysicalSize(1240, 860));
  native.window.scaleFactor.mockResolvedValue(1);
  native.window.setMinSize.mockResolvedValue(undefined);
  native.window.setSize.mockResolvedValue(undefined);
});

afterEach(() => vi.unstubAllGlobals());

describe("layout preference", () => {
  it.each([null, "expanded", "unknown", "", "COLLAPSED"])(
    "defaults to expanded for %s",
    (saved) => {
      vi.stubGlobal("localStorage", { getItem: () => saved });
      expect(loadLayout()).toBe("expanded");
    },
  );

  it("restores the compact layout and saves only a local UI preference", () => {
    const storage = { getItem: vi.fn(() => "collapsed"), setItem: vi.fn() };
    vi.stubGlobal("localStorage", storage);
    expect(loadLayout()).toBe("collapsed");
    expect(storage.getItem).toHaveBeenCalledWith("lobbylocker.layout");
    saveLayout("expanded");
    expect(storage.setItem).toHaveBeenCalledWith(
      "lobbylocker.layout",
      "expanded",
    );
    expect(native.getCurrentWindow).not.toHaveBeenCalled();
  });

  it("still works when storage is missing or denied", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(loadLayout()).toBe("expanded");
    expect(() => saveLayout("collapsed")).not.toThrow();
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("Storage denied");
      },
      setItem: () => {
        throw new Error("Storage denied");
      },
    });
    expect(loadLayout()).toBe("expanded");
    expect(() => saveLayout("collapsed")).not.toThrow();
  });
});

describe("native window layout", () => {
  it("lowers the minimum before collapsing and restores the expanded size", async () => {
    const layout = new WindowLayout();
    await layout.set("collapsed");
    expect(native.window.setMinSize).toHaveBeenLastCalledWith(
      new LogicalSize(360, 560),
    );
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(420, 800),
    );
    expect(native.window.setMinSize.mock.invocationCallOrder[0]).toBeLessThan(
      native.window.setSize.mock.invocationCallOrder[0],
    );

    native.window.innerSize.mockResolvedValue(new PhysicalSize(420, 800));
    await layout.set("expanded");
    expect(native.window.setMinSize).toHaveBeenLastCalledWith(
      new LogicalSize(760, 560),
    );
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(1240, 860),
    );
  });

  it("remembers manually resized windows in logical pixels on HiDPI displays", async () => {
    const layout = new WindowLayout();
    native.window.innerSize.mockResolvedValue(new PhysicalSize(2000, 1400));
    native.window.scaleFactor.mockResolvedValue(2);
    await layout.set("collapsed");

    native.window.innerSize.mockResolvedValue(new PhysicalSize(800, 1200));
    await layout.set("expanded");
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(1000, 700),
    );

    native.window.innerSize.mockResolvedValue(new PhysicalSize(2000, 1400));
    await layout.set("collapsed");
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(400, 600),
    );
  });

  it("does not resize an unchanged layout or a browser-only preview", async () => {
    const layout = new WindowLayout();
    await layout.set("expanded");
    expect(native.getCurrentWindow).not.toHaveBeenCalled();
    await layout.set("collapsed");
    await layout.set("collapsed");
    expect(native.window.setSize).toHaveBeenCalledOnce();

    native.isTauri.mockReturnValue(false);
    await layout.set("expanded");
    expect(native.window.setSize).toHaveBeenCalledOnce();
  });

  it("restores size constraints after a failed resize and allows a retry", async () => {
    const layout = new WindowLayout();
    native.window.setSize.mockRejectedValueOnce(new Error("Resize denied"));
    await expect(layout.set("collapsed")).rejects.toThrow("Resize denied");
    expect(native.window.setMinSize.mock.calls.map(([size]) => size)).toEqual([
      new LogicalSize(360, 560),
      new LogicalSize(760, 560),
    ]);
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(1240, 860),
    );

    await layout.set("collapsed");
    expect(native.window.setSize).toHaveBeenLastCalledWith(
      new LogicalSize(420, 800),
    );
  });

  it("does not request a resize when reading or changing constraints fails", async () => {
    const layout = new WindowLayout();
    native.window.innerSize.mockRejectedValueOnce(new Error("Read failed"));
    await expect(layout.set("collapsed")).rejects.toThrow("Read failed");
    expect(native.window.setMinSize).not.toHaveBeenCalled();
    native.window.setMinSize.mockRejectedValueOnce(new Error("Resize denied"));
    await expect(layout.set("collapsed")).rejects.toThrow("Resize denied");
    expect(native.window.setSize).not.toHaveBeenCalled();
  });
});
