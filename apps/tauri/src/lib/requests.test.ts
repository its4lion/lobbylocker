import { afterEach, describe, expect, it, vi } from "vitest";
import { Requests } from "./requests";

afterEach(() => vi.useRealTimers());

describe("request scheduling", () => {
  it("serializes icon loading with mutations instead of racing the busy guard", async () => {
    let complete!: (value: unknown) => void;
    const invoke = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            complete = resolve;
          }),
      )
      .mockResolvedValue(undefined);
    const requests = new Requests(invoke);
    const icons = requests.request("get_custom_icons");
    const change = requests.request("set_preferences", { preferences: {} });
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("get_custom_icons", undefined);
    complete({ example: "icon" });
    expect(await icons).toEqual({ example: "icon" });
    await change;
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "get_custom_icons",
      "set_preferences",
    ]);
  });

  it("a failed request does not block the next one", async () => {
    const invoke = vi
      .fn()
      .mockRejectedValueOnce(new Error("Read failed"))
      .mockResolvedValue("ok");
    const requests = new Requests(invoke);
    const failed = requests.request("get_custom_icons");
    const next = requests.request("scan_games");
    await expect(failed).rejects.toThrow("Read failed");
    expect(await next).toBe("ok");
  });

  it.each([
    "Another operation is running. Please wait.",
    new Error("Another operation is running. Please wait."),
  ])(
    "waits for pre-execution guard rejection without exposing it",
    async (error) => {
      vi.useFakeTimers();
      const invoke = vi
        .fn()
        .mockRejectedValueOnce(error)
        .mockResolvedValue("done");
      const request = new Requests(invoke).request("apply_rules");
      await vi.advanceTimersByTimeAsync(249);
      expect(invoke).toHaveBeenCalledOnce();
      await vi.advanceTimersByTimeAsync(1);
      expect(await request).toBe("done");
      expect(invoke).toHaveBeenCalledTimes(2);
    },
  );

  it.each([
    "Authorization cancelled",
    "Helper failed",
    "Partial firewall update",
  ])(
    "never retries %s or risks a duplicate firewall operation",
    async (message) => {
      const invoke = vi.fn().mockRejectedValue(new Error(message));
      await expect(new Requests(invoke).request("apply_rules")).rejects.toThrow(
        message,
      );
      expect(invoke).toHaveBeenCalledOnce();
    },
  );

  it("bounds the wait and replaces a persistent internal rejection with a useful message", async () => {
    vi.useFakeTimers();
    const invoke = vi
      .fn()
      .mockRejectedValue("Another operation is running. Please wait.");
    const request = new Requests(invoke).request("scan_games");
    const assertion = expect(request).rejects.toThrow(
      "The previous action is still finishing. Try again in a moment.",
    );
    await vi.advanceTimersByTimeAsync(30_000);
    await assertion;
    expect(invoke).toHaveBeenCalledTimes(121);
  });
});
