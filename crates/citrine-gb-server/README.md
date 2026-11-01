# Citrine server

One binary that serves the web app, runs the online relay, or both, with an admin panel at
`/admin`. It reads its settings from environment variables and from a `.env` file in its working
directory; a variable set in the environment wins over the file.
[docker/.env.example](docker/.env.example) lists every variable with what it does.

```sh
cd crates/citrine-gb-server/docker
cp .env.example .env
docker compose up -d --build
```

To run it from source, use `cargo run --release` in `crates/citrine-gb-server/`. It serves the web app from
`./static`, which Docker builds for you; without Docker, build it first in `crates/citrine-gb-app/` with
`trunk build --release --dist ../citrine-gb-server/static`.

## The four setups

**Web app only** (the default). `CITRINE_WEB=true`, `CITRINE_RELAY=false`. The server hosts Citrine
Web from `CITRINE_STATIC_DIR` and nothing else.

**Relay only.** `CITRINE_WEB=false`, `CITRINE_RELAY=true` and a `CITRINE_RELAY_PASSWORD`. People
point the app at this server in Settings → Online and join with the password. `/` returns 404.

**Both.** `CITRINE_WEB=true`, `CITRINE_RELAY=true` and a password. One address serves the app and
the relay.

**None.** `CITRINE_WEB=false`, `CITRINE_RELAY=false`. Only `/admin` works, if
`CITRINE_ADMIN_PASSWORD` is set, and `/` shows a page that says what to switch on. Useful to
look at the data or turn the relay on from the panel.

The relay never runs open by accident: with the relay on and no password the server refuses to
start, unless `CITRINE_RELAY_OPEN=true`.

## Playing together

Friends in a session send each other only their button presses, sealed so the relay can't read
them, and each side runs both Game Boys. That needs both games on each side. With
`CITRINE_GAME_SHARING=true` the relay passes a game file from the friend who has it to the one
who doesn't, in memory only, so one copy is enough. It is `false` by default, since it risks
distributing games: weigh it for the context your relay runs in. With it off, friends without
each other's game still trade and battle over a slower link cable.

## Admin panel

Set `CITRINE_ADMIN_PASSWORD` to turn it on. It shows who is online, accounts, sessions, storage
and traffic, and has the relay switch, a kill switch that disconnects everyone until it's turned
off, the relay password and the limits per user. Settings changed there are saved in the database
and win over the environment until they are reset in the panel.

Two-factor sign-in is set up in the panel under Settings: scan the QR code with an authenticator
app, enter the code it shows, and save the 10 recovery codes. From then on, signing in asks for the
password and a code from the app, or a recovery code, which works once. Turning it off asks for a
code.

Anyone with the relay password can make an account. To choose who joins, turn on "New accounts
need an invite code" in Settings, then make a code for each person on the Invites page, with a
note on who it's for. A code makes one account; the page shows who used each one and when, and an
unused code can be revoked. Existing accounts keep working either way.

Lost the phone and the recovery codes? Start the server once with `CITRINE_ADMIN_2FA_RESET=true`.
It turns two-factor sign-in off at start and logs that it did. Then remove the variable, sign in
with the password and set it up again. While the variable is set, every start turns it off again.

## Data

Accounts, friends, waiting chat messages, invite codes, settings and the admin panel's two-factor key live in
`citrine.db` in `CITRINE_DATA_DIR`,
shared files in its `files` folder. The server only stores encrypted messages and files and the
hash of each account's key, never the relay ID the keys come from. Messages nobody fetched are
deleted after 30 days.

Once a day, at start and in the first hour after midnight UTC, the server copies the database to
its `backups` folder as `citrine-YYYY-MM-DD.db` and keeps the 14 newest. Back up `backups` and
`files`, not the live `citrine.db`: a copy taken while it is written can be torn. Files are written
once and never changed, so they copy safely while the server runs. To restore, stop the server and
put a copy back as `citrine.db`.

## Behind a reverse proxy

The relay and admin panel limit wrong passwords per address. Behind a proxy every visitor arrives
from the proxy's address, so one person's wrong guesses lock everyone out for a minute. Set
`CITRINE_TRUSTED_PROXY_HOPS` to the number of proxies of your own in front of the server, usually
1: the server then takes the visitor's address from `X-Forwarded-For`, that many entries from the
right, or from `X-Real-IP`. Leave it at 0 without a proxy, since anyone can send those headers.

## Changing the database

The schema is the numbered files in `migrations/`. At start the server applies the ones the
database hasn't had yet. Queries use sqlx's `query!` macros, which check them against the schema
saved in `.sqlx/` at build time; `.cargo/config.toml` sets `SQLX_OFFLINE=true`, so every build
reads `.sqlx/` and needs no database.

To add a migration, write the next file (`0003_<what>.sql`); never edit one that has shipped,
since the server refuses to start when an applied file changed. After a new migration or a changed
query, refresh `.sqlx/` and commit it:

```sh
cargo install sqlx-cli --no-default-features --features sqlite
cd server
export DATABASE_URL=sqlite:/tmp/citrine-dev.db
rm -f /tmp/citrine-dev.db && sqlx database create && sqlx migrate run
cargo sqlx prepare -- --all-targets
```

The admin panel uses icons from [Lucide](https://lucide.dev), under the ISC licence in `src/admin/LICENSE-lucide.txt`.
