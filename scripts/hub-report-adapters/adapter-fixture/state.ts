import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";

export const HOME = process.env.AUTOPILOT_HOME ?? `${process.env.HOME}/.ferryx/hub/workspace/outsourcing-autopilot`;
mkdirSync(HOME, { recursive: true });

export type Kind = "kmong_message" | "kmong_order" | "wishket_project" | "freelancer_project" | "freemoa_project" | "mercor_project";
export type Status =
  | "new" | "skipped" | "pending_approval" | "approved" | "rejected" | "executing" | "executed" | "failed";

export interface Item {
  id: string;
  kind: Kind;
  external_id: string;
  url: string;
  title: string;
  body: string;
  verdict: string | null;
  draft: string | null;
  status: Status;
  chat_ref: string | null;
  result: string | null;
  created_at: string;
  updated_at: string;
}

export const db = new Database(`${HOME}/state.db`, { create: true });
db.exec(`PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, external_id TEXT NOT NULL, url TEXT NOT NULL,
  title TEXT NOT NULL, body TEXT NOT NULL, verdict TEXT, draft TEXT, status TEXT NOT NULL,
  chat_ref TEXT, result TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS log (at TEXT NOT NULL, item_id TEXT, event TEXT NOT NULL, detail TEXT);`);

const now = () => new Date().toISOString();

export function upsertNew(i: Pick<Item, "id" | "kind" | "external_id" | "url" | "title" | "body">): boolean {
  const r = db.query(`INSERT OR IGNORE INTO items (id,kind,external_id,url,title,body,status,created_at,updated_at)
    VALUES ($id,$kind,$external_id,$url,$title,$body,'new',$t,$t)`).run({ ...prefix(i), $t: now() });
  return r.changes > 0;
}

export function get(id: string): Item | null {
  return db.query(`SELECT * FROM items WHERE id=$id`).get({ $id: id }) as Item | null;
}

export function byShort(short: string): Item | null {
  return db.query(`SELECT * FROM items WHERE rowid=$r`).get({ $r: Number(short) }) as Item | null;
}

export function shortId(id: string): string {
  const r = db.query(`SELECT rowid AS r FROM items WHERE id=$id`).get({ $id: id }) as { r: number } | null;
  if (!r) throw new Error(`unknown item ${id}`);
  return String(r.r);
}

export function update(id: string, fields: Partial<Item>) {
  const keys = Object.keys(fields);
  if (!keys.length) return;
  const sets = keys.map((k) => `${k}=$${k}`).join(",");
  db.query(`UPDATE items SET ${sets}, updated_at=$updated WHERE id=$id`).run({ ...prefix(fields), $updated: now(), $id: id });
}

export function listByStatus(status: Status): Item[] {
  return db.query(`SELECT * FROM items WHERE status=$s ORDER BY created_at`).all({ $s: status }) as Item[];
}

export function log(itemId: string | null, event: string, detail = "") {
  db.query(`INSERT INTO log VALUES ($at,$i,$e,$d)`).run({ $at: now(), $i: itemId, $e: event, $d: detail.slice(0, 4000) });
}

function prefix(o: Record<string, unknown>) {
  return Object.fromEntries(Object.entries(o).map(([k, v]) => [`$${k}`, v ?? null]));
}
