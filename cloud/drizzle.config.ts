import { defineConfig } from "drizzle-kit";

export default defineConfig({
  schema: "./src/db/schema.ts",
  out: "./drizzle",
  dialect: "postgresql",
  dbCredentials: {
    // drizzle-kit validates this block even for `generate`, which never opens a
    // connection. The placeholder keeps migration generation working on a
    // machine with no .env.local; `db:push` and `db:studio` do connect and get
    // the real URL from dotenv-cli (see the npm scripts).
    url: process.env.DATABASE_URL ?? "postgres://localhost:5432/hangar_cloud",
  },
  strict: true,
  verbose: true,
});
