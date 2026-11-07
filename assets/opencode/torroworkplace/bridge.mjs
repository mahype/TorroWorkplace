// Reporting-only OpenCode V2 event bridge. Never answers a form or permission request.
import { spawn } from "node:child_process";

export const types = ["permission.asked", "permission.replied", "form.created", "form.replied",
  "form.cancelled", "session.execution.started", "session.execution.succeeded", "session.deleted"];

export async function send(program, keyFile, payload, signal, timeout = 2000) {
  const input = JSON.stringify(payload);
  if (Buffer.byteLength(input) > 32 * 1024 || signal?.aborted) return;
  await new Promise((resolve) => {
    let child;
    try {
      child = spawn(program, ["hook", "opencode", "--key-file", keyFile], {
        shell: false, stdio: ["pipe", "ignore", "ignore"], windowsHide: true,
      });
    } catch { resolve(); return; }
    const stop = () => { child.kill("SIGKILL"); };
    const timer = setTimeout(stop, timeout);
    const done = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", stop);
      resolve();
    };
    signal?.addEventListener("abort", stop, { once: true });
    child.once("error", done);
    child.once("close", done);
    child.stdin.on("error", () => {});
    child.stdin.end(input);
    if (signal?.aborted) stop();
  });
}

export function bridge({ directory, session, report, signal, timeout = 2000 }) {
  const sessions = new Map();
  const seen = new Set();
  let closed = false;
  const clean = () => { closed = true; sessions.clear(); seen.clear(); };
  signal?.addEventListener("abort", clean, { once: true });
  const bounded = async (work) => {
    let timer;
    try {
      return await Promise.race([work, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("timeout")), timeout);
      })]);
    } finally { clearTimeout(timer); }
  };
  return async (event) => {
    if (closed || signal?.aborted || !types.includes(event?.type)) return;
    // Global plugins may be instantiated for several Locations on the same server. Dedupe alone
    // is local to this instance; never act on an explicitly different source Location.
    if (directory && event.location?.directory && event.location.directory !== directory) return;
    const data = event.data;
    const form = data?.form;
    const id = form?.sessionID ?? data?.sessionID;
    if (typeof id !== "string" || !id || id.length > 200 || typeof event.id !== "string") return;
    if (seen.has(event.id)) return;
    seen.add(event.id);
    if (seen.size > 4096) seen.delete(seen.values().next().value);
    let state = sessions.get(id);
    if (!state) {
      if (sessions.size >= 1024) return;
      state = { pending: new Set(), tail: Promise.resolve(), info: undefined, queued: 0 };
      sessions.set(id, state);
    }
    const run = async () => {
      if (closed || signal?.aborted) return;
      try {
        if (!state.info) {
          try {
            const result = await bounded(session(id, signal));
            // The Promise plugin context exposes Session.Info; HTTP-backed adapters may retain
            // the documented {data: Session.Info} envelope. Normalize before checking parent/scope.
            state.info = result?.data ?? result;
          }
          catch {
            // A deleted session is no longer queryable (also after plugin reload). Resolving its
            // own thread is harmless for an ignored child and cannot resolve a parent's thread.
            if (event.type !== "session.deleted") {
              return;
            }
            state.info = {};
          }
        }
        const info = state.info;
        if ((directory && info?.location?.directory && info.location.directory !== directory) || info?.parentID) {
          return;
        }
        if (!info || closed || signal?.aborted) return;
        let intent;
        switch (event.type) {
          case "permission.asked":
          case "form.created": {
            const request = form?.id ?? data.id;
            if (typeof request !== "string" || !request) return;
            const key = `${form ? "form" : "permission"}:${request}`;
            if (state.pending.has(key)) return;
            state.pending.add(key);
            intent = "waiting";
            break;
          }
          case "permission.replied":
          case "form.replied":
          case "form.cancelled": {
            const permission = event.type === "permission.replied";
            const request = permission ? data.requestID : data.id;
            if (!state.pending.delete(`${permission ? "permission" : "form"}:${request}`)) return;
            if (state.pending.size) return;
            intent = "resolved";
            break;
          }
          case "session.execution.started": state.pending.clear(); intent = "started"; break;
          case "session.execution.succeeded":
            if (state.pending.size) return;
            intent = "done";
            break;
          case "session.deleted": state.pending.clear(); intent = "deleted"; break;
        }
        const payload = {
          hook_event_name: intent, session_id: id,
          title: typeof info.title === "string" ? info.title.slice(0, 500) : undefined,
          cwd: info.location?.directory,
          message: intent === "waiting" ? String(form?.title ?? data.message ?? data.action ?? "").slice(0, 4000) : undefined,
        };
        if (!closed && !signal?.aborted) await report(payload, signal);
      } catch { /* A failed notification must never interfere with OpenCode. */ }
    };
    state.queued++;
    state.tail = state.tail.then(run, run).finally(() => {
      state.queued--;
      // Keep one tail until all queued work drains, including failed lookups and deletion.
      // Cache only live questions and queued work, not every completed historical session.
      if (!state.queued && !state.pending.size && sessions.get(id) === state) sessions.delete(id);
    });
    return state.tail;
  };
}
