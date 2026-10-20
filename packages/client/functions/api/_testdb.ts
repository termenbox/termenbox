import { readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { D1Like } from './directory/_lib';

// Vite does not know node:sqlite as a builtin yet, so load it through require.
const { DatabaseSync } = createRequire(import.meta.url)('node:sqlite') as typeof import('node:sqlite');

/** A D1-shaped wrapper over node:sqlite, loaded with the real migrations in order. */
export function testDb(): D1Like {
  const sqlite = new DatabaseSync(':memory:');
  const dir = new URL('../../migrations/', import.meta.url);
  for (const file of readdirSync(dir).sort()) sqlite.exec(readFileSync(new URL(file, dir), 'utf8'));
  return {
    prepare: (sql: string) => ({
      bind: (...args: unknown[]) => ({
        sql,
        args,
        first: async <T,>() => ((sqlite.prepare(sql).get(...(args as never[])) as T | undefined) ?? null),
        run: async () => sqlite.prepare(sql).run(...(args as never[])),
        all: async <T,>() => ({ results: sqlite.prepare(sql).all(...(args as never[])) as T[] }),
      }),
    }),
    // Like D1's batch: one transaction, rolled back if any statement fails.
    batch: async (statements: unknown[]) => {
      sqlite.exec('BEGIN');
      try {
        const out = (statements as { sql: string; args: unknown[] }[]).map((s) => sqlite.prepare(s.sql).run(...(s.args as never[])));
        sqlite.exec('COMMIT');
        return out;
      } catch (e) {
        sqlite.exec('ROLLBACK');
        throw e;
      }
    },
  };
}
