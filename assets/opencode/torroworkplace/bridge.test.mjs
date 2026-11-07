import { test } from "node:test";
import assert from "node:assert/strict";
import { bridge, send } from "./bridge.mjs";

function room() {
  const reports = [];
  const controller = new AbortController();
  const receive = bridge({
    signal: controller.signal,
    session: async (id) => ({ title: "Task", parentID: id === "child" ? "parent" : undefined, location: { directory: "/work" } }),
    report: async (payload) => reports.push(payload),
  });
  let n = 0;
  const event = (type, data) => ({ type, id: `event-${++n}`, data });
  return { receive, reports, controller, event };
}

test("open questions are isolated, deduplicated and resolved only after the last answer", async () => {
  const r = room();
  const ask = r.event("permission.asked", { sessionID: "s", id: "p", action: "edit" });
  await r.receive(ask);
  await r.receive(ask);
  await r.receive(r.event("form.created", { form: { sessionID: "s", id: "f", title: "Which?" } }));
  await r.receive(r.event("permission.replied", { sessionID: "s", requestID: "p" }));
  assert.equal(r.reports.length, 2);
  await r.receive(r.event("form.cancelled", { sessionID: "s", id: "f" }));
  assert.deepEqual(r.reports.map((p) => p.hook_event_name), ["waiting", "waiting", "resolved"]);
});

test("child sessions are ignored; start, success and deletion report without an answer", async () => {
  const r = room();
  await r.receive(r.event("session.execution.succeeded", { sessionID: "child" }));
  for (const type of ["session.execution.started", "session.execution.succeeded", "session.deleted"]) {
    await r.receive(r.event(type, { sessionID: "s" }));
  }
  assert.deepEqual(r.reports.map((p) => p.hook_event_name), ["started", "done", "deleted"]);
  r.controller.abort();
  await r.receive(r.event("session.execution.succeeded", { sessionID: "s" }));
  assert.equal(r.reports.length, 3);
});

test("concurrent sessions do not block, while one session remains serialized", async () => {
  const reports = [];
  let release;
  const blocked = new Promise((resolve) => { release = resolve; });
  const receive = bridge({
    session: async (id) => { if (id === "a") await blocked; return {}; },
    report: async (p) => reports.push(`${p.session_id}:${p.hook_event_name}`),
  });
  const event = (id, type, sessionID) => ({ id, type, data: { sessionID } });
  const first = receive(event("1", "session.execution.started", "a"));
  const second = receive(event("2", "session.execution.succeeded", "a"));
  await receive(event("3", "session.execution.succeeded", "b"));
  assert.deepEqual(reports, ["b:done"]);
  release();
  await Promise.all([first, second]);
  assert.deepEqual(reports, ["b:done", "a:started", "a:done"]);
});

test("missing program, oversized payload and a timed-out session lookup are harmless", async () => {
  await send("/not/a/program", "/no/key", {});
  await send("/not/a/program", "/no/key", { message: "x".repeat(40000) });
  const receive = bridge({ session: () => new Promise(() => {}), report: () => assert.fail("no report"), timeout: 5 });
  await receive({ id: "e", type: "session.execution.succeeded", data: { sessionID: "s" } });
});

test("deletion after plugin reload resolves even when the session API no longer finds it", async () => {
  const reports = [];
  const receive = bridge({ session: async () => { throw new Error("not-found"); }, report: async (p) => reports.push(p) });
  await receive({ id: "deleted", type: "session.deleted", data: { sessionID: "gone" } });
  assert.equal(reports.length, 1);
  assert.equal(reports[0].hook_event_name, "deleted");
  assert.equal(reports[0].session_id, "gone");
});

test("global plugin instances ignore another Location, including deletion without a lookup", async () => {
  const reports = [];
  const receive = (directory) => bridge({ directory,
    session: async () => ({ location: { directory: "/a" } }),
    report: async (p) => reports.push(`${directory}:${p.hook_event_name}`),
  });
  const a = receive("/a"), b = receive("/b");
  const event = { id: "asked", type: "permission.asked", location: { directory: "/a" }, data: { sessionID: "s", id: "p" } };
  await Promise.all([a(event), b(event)]);
  assert.deepEqual(reports, ["/a:waiting"]);
  await b({ id: "deleted", type: "session.deleted", location: { directory: "/a" }, data: { sessionID: "s" } });
  assert.equal(reports.length, 1);
  // A transient event without Location is still scoped by its required Session.Info location.
  await b({ id: "no-location", type: "session.execution.succeeded", data: { sessionID: "s" } });
  assert.equal(reports.length, 1);
});

test("foreign, child and failed session lookups cannot permanently exhaust the cache", async () => {
  const reports = [];
  const receive = bridge({ directory: "/own",
    session: async (id) => {
      if (id.startsWith("failed")) throw new Error("not-found");
      return { parentID: id.startsWith("child") ? "parent" : undefined,
        location: { directory: id.startsWith("foreign") ? "/foreign" : "/own" } };
    },
    report: async (p) => reports.push(p),
  });
  for (const prefix of ["foreign", "child", "failed"]) {
    for (let n = 0; n < 1024; n++) {
      await receive({ id: `${prefix}-${n}`, type: "session.execution.succeeded", data: { sessionID: `${prefix}-${n}` } });
    }
  }
  await receive({ id: "own", type: "session.execution.succeeded", location: { directory: "/own" }, data: { sessionID: "own" } });
  assert.equal(reports.length, 1);
  assert.equal(reports[0].session_id, "own");
});

test("HTTP-backed Session.Info envelopes still enforce parent and Location checks", async () => {
  const reports = [];
  const receive = bridge({ directory: "/a",
    session: async (id) => ({ data: { parentID: id === "child" ? "p" : undefined,
      title: "Task", location: { directory: id === "foreign" ? "/b" : "/a" } } }),
    report: async (p) => reports.push(p),
  });
  for (const id of ["child", "foreign", "own"]) {
    await receive({ id, type: "session.execution.succeeded", data: { sessionID: id } });
  }
  assert.equal(reports.length, 1);
  assert.equal(reports[0].title, "Task");
  assert.equal(reports[0].cwd, "/a");
});

test("failed lookup keeps queued events on the same session tail", async () => {
  const reports = [];
  let release, entered;
  const blocked = new Promise((resolve) => { release = resolve; });
  const lookupEntered = new Promise((resolve) => { entered = resolve; });
  let calls = 0;
  const receive = bridge({
    session: async () => {
      if (++calls === 1) throw new Error("transient");
      if (calls === 2) { entered(); await blocked; }
      return {};
    },
    report: async (p) => reports.push(p.hook_event_name),
  });
  const event = (id, type) => ({ id, type, data: { sessionID: "s", id: "p" } });
  const oldStart = receive(event("1", "session.execution.started"));
  const oldDone = receive(event("2", "session.execution.succeeded"));
  await oldStart;
  await lookupEntered;
  const newStart = receive(event("3", "session.execution.started"));
  const newQuestion = receive(event("4", "permission.asked"));
  await new Promise(setImmediate);
  release();
  await Promise.all([oldDone, newStart, newQuestion]);
  assert.deepEqual(reports, ["done", "started", "waiting"]);
});
