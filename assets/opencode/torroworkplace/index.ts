// Torro Workplace reporting-only OpenCode V2 plugin. Generated paths contain no token.
import { Plugin } from "@opencode/plugin";
import { bridge, send, types } from "./bridge.mjs";

export default Plugin.define({
  id: "torroworkplace.inbox",
  async setup(ctx) {
    const controller = new AbortController();
    const program = __TORRO_PROGRAM__;
    const keyFile = __TORRO_KEY_FILE__;
    const pendingKey = (sessionID: string) =>
      `pending/${encodeURIComponent(ctx.location.directory)}/${encodeURIComponent(sessionID)}`;
    const receive = bridge({
      directory: ctx.location.directory,
      signal: controller.signal,
      session: (sessionID, signal) => ctx.session.get({ sessionID }, { signal }),
      loadPending: (sessionID) => ctx.storage.get(pendingKey(sessionID)).then((value) => value ?? []),
      savePending: (sessionID, pending) => pending.length
        ? ctx.storage.set(pendingKey(sessionID), pending)
        : ctx.storage.remove(pendingKey(sessionID)),
      report: (payload, signal) => send(program, keyFile, payload, signal),
    });
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({ types, signal: controller.signal })) {
          // Each session has its own serial queue; unrelated sessions never block each other.
          void receive(event);
        }
      } catch { /* Unloading or an unavailable event stream is harmless. */ }
    })();
    return async () => {
      controller.abort();
      await receive.drain();
    };
  },
});
