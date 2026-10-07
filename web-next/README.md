# Web UI (Next.js browse)

Next.js browse UI restored from git history (`7a1b6be`, handles-schema tip).
It is not the product: the product GUI is the Vite SPA in [`../web/`](../web/)
(Tauri + Docker static). This tree stays so its screens can be run against a
live vault and judged for what is worth porting into `web/`.

It reads the vault through the HTTP API (`/v1`), the same way `web/` does.
Every read in `src/lib/vault/` calls the vault server; nothing opens the
database file. Writes are not mapped yet: every route handler that would
change data answers 501, and the screens show their error state.

Product documentation:

**https://bitrealm.io/vault/user/browse-your-messages/**

## Local development

From the repo root, start a vault with demo data, then run this app against
it:

```bash
./scripts/run-vault-dev.sh --reset-demo   # vault API on http://127.0.0.1:8080

cd web-next
npm ci
npm run dev                               # http://127.0.0.1:3000
```

Sign in with user `demo` and an empty password. The vault host comes from
`VAULT_API_URL` (default `http://127.0.0.1:8080`).

### Scripts

| Script | Purpose |
|--------|---------|
| `npm run dev` | Next.js dev server |
| `npm run build` / `start` | Production build / serve (`output: "standalone"`) |
| `npm run lint` | ESLint |
| `npm test` | `tsx --test src/**/*.test.ts` |
| `npm run gen:api` | Regenerate `src/lib/vault/types.generated.ts` from `docs/src/assets/openapi.json` |

### How it is wired

- `src/lib/vault/client.ts` — fetch wrapper: base URL, Bearer token from the
  `mv_session` cookie, paging helpers, a few-second read cache.
- `src/lib/vault/{account,labels,contacts,conversations,messages,search,home}.ts`
  — the reads each screen needs, shaped into the row types in `src/lib/types.ts`.
- `src/lib/db.ts` — the barrel pages and route handlers import; it re-exports
  the modules above.
- `src/app/api/auth/login` — `POST /v1/auth/login`; the token and account id
  go in httpOnly cookies. Logout revokes the token.
- `src/app/api/assets/[source]/[sha256]` — proxies `GET /v1/assets/{sha256}`.
- Display preferences live in a cookie (`src/lib/vault/prefs.ts`): the vault
  has no preferences route.

### What is not wired

The SQL implementation the app used before (`src/lib/*Read.ts`, `dbCore.ts`,
`vaultSchema.generated.ts`, the `*Write.ts` modules and their tests) is still
in the tree and no longer called from the read path. Undo and redo are not
features of this app; the history engine under `src/components/history/`
is unwired. Screens whose data the API does not expose (transcoded media,
display preferences, unassigned handles, contact CSV export, demo reset)
show a "no /v1 route" notice or answer 501. The gap list lives in
the repository's issue tracker.
