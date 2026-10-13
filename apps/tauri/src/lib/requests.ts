type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

const busyMessage = "Another operation is running. Please wait.";
const waitMessage =
  "The previous action is still finishing. Try again in a moment.";

/** Serialize calls that share the backend busy guard, including icon reads.
 * Only a guard rejection BEFORE work starts is safe to retry. Never replay
 * administrator cancellations, helper failures, or other mutation errors.
 */
export class Requests {
  private tail: Promise<unknown> = Promise.resolve();

  constructor(private invoke: Invoke) {}

  request<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const request = this.tail.then(() => this.send<T>(command, args));
    this.tail = request.catch(() => {});
    return request;
  }

  private async send<T>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T> {
    for (let attempt = 0; ; attempt++) {
      try {
        return await this.invoke<T>(command, args);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (message !== busyMessage) throw error;
        // A previous call may still be finishing. Keep the caller's busy state
        // while waiting, but never expose the internal guard message.
        if (attempt >= 120) throw new Error(waitMessage);
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
    }
  }
}
