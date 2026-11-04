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
distributing games: weigh it for the context your relay runs in. Even when it is `true`, the relay
passes games on only while new accounts need an invite code, set with `CITRINE_INVITES=true` or
the switch in the panel, so strangers on an open relay can't swap games through it. With it off, friends without each other's game still trade and battle over
a slower link cable.

## Admin panel

Set `CITRINE_ADMIN_PASSWORD` to turn it on. It shows who is online, accounts, sessions, storage,
the database's size, waiting chat messages, traffic and how many requests each limit refused in
the last hour. It has the relay switch, a kill switch that disconnects everyone until it's turned
off, the relay password, the limits per user and the total storage. The Storage tile counts files
and the database against the total storage, or against the free disk above the floor when that is
less. Settings changed there are saved in the database and win over the environment until they are
reset in the panel.

The Users page lists accounts by last seen or newest first, with when each one joined. Each
account can be disconnected, banned, have its files deleted or be deleted with everything it
owns. A banned account's friend requests go and its files stop downloading. "Ban new accounts"
bans every account that joined in the last hour, day or week, for a flood of sign-ups. For an
account that is online, the page shows its address, and "Block address for 24 h" disconnects
everyone on it and refuses new connections from it for a day; IPv6 addresses block by their /64.
Addresses and blocks are kept in memory only, never in the database or the log, and a restart
clears them.

"Delete empty accounts" in Settings, off by default, deletes once an hour every account nobody
signed in to for 30 days that has no friends, files or cloud saves. Banned accounts stay.

Two-factor sign-in is set up in the panel under Settings: scan the QR code with an authenticator
app, enter the code it shows, and save the 10 recovery codes. From then on, signing in asks for the
password and a code from the app, or a recovery code, which works once. Turning it off asks for a
code.

Anyone with the relay password can make an account. To choose who joins, turn on "New accounts
need an invite code" in Settings or start with `CITRINE_INVITES=true`, then make a code for each person on the Invites page, with a
note on who it's for. A code makes one account; the page shows who used each one and when, and an
unused code can be revoked. Existing accounts keep working either way.

Lost the phone and the recovery codes? Start the server once with `CITRINE_ADMIN_2FA_RESET=true`.
It turns two-factor sign-in off at start and logs that it did. Then remove the variable, sign in
with the password and set it up again. While the variable is set, every start turns it off again.

Accounts can turn on two-factor sign-in in the app too. Then signing in with the relay ID on a
device the account doesn't know asks for a code, and so do turning it off, making new recovery
codes and deleting the account. A user who lost both the phone and the recovery codes asks you:
"Turn off two-factor" in the user's Manage menu turns it off, and the devices that account already
knows keep signing in.

## Relay key

The relay makes a key on its first start and keeps it in the database. Every connection starts
with a Noise handshake (`Noise_NX_25519_ChaChaPoly_BLAKE2s`) that proves the relay holds it, and
every message after that is encrypted, a reverse proxy in front included. The app remembers the
key when an account first joins and refuses to connect if the relay later shows another one.

The log prints the key's fingerprint at start, and the dashboard shows it. With
`CITRINE_PUBLIC_URL` set, its button "Copy with address" copies the address with the key, as in
`https://gb.lemon.industries#K7QM2XRA91FDTPWE`. An app joining with that address checks the key on
its first connect too. Restoring the database keeps
the key; a fresh database makes a new one, and every app asks its user to accept it.

## Limits

The relay caps what one account, one connection or one address can use, so a single person
with a script can't fill the disk or the memory, or lock other people out. The defaults suit a
relay open to anyone; each one has a variable to change it, listed with its default.

| Variable | Default | What it caps |
| --- | --- | --- |
| `CITRINE_QUEUE_MB` | 8 | Data waiting to go out to one connection. A friend's message that doesn't fit closes the slow connection |
| `CITRINE_SEND_TIMEOUT_SECS` | 20 | How long a send may wait on a connection that stopped reading before it closes |
| `CITRINE_DOWNLOADS_AT_ONCE` | 2 | Files one connection downloads at the same time |
| `CITRINE_DOWNLOADS_WAITING` | 16 | Downloads that wait their turn; more are refused |
| `CITRINE_STORAGE_MB` | 100, or 20 on a relay open to anyone | Storage per account. 0 turns file sharing and cloud saves off. On an open relay with no password and invites off, the default is 20 |
| `CITRINE_STORAGE_TOTAL_MB` | 10240 | Storage of all accounts together, database included. Uploads, saves and chat messages past it are refused. "Total storage" in the panel's Settings wins over it |
| `CITRINE_DISK_FLOOR_MB` | 2048 | Free disk the relay always leaves. Writes stop when the disk holding `CITRINE_DATA_DIR` has less free |
| `CITRINE_DISK_FLOOR_PERCENT` | 5 | The same floor as a share of the disk; the larger of the two counts |
| `CITRINE_MAX_FILES` | 1000 | Shared files per account |
| `CITRINE_MAX_SAVES` | 500 | Cloud save slots per account |
| `CITRINE_INBOX_MESSAGES` | 2000 | Chat messages one friend can leave waiting for another who is offline |
| `CITRINE_INBOX_MB` | 32 | The same in MB |
| `CITRINE_MAX_CONNECTIONS` | 500 | Connections to the relay at once. More get "the relay is full" |
| `CITRINE_IP_CONNECTIONS` | 10 | Connections from one address at once. IPv6 addresses count by their /64 |
| `CITRINE_IP_UNSIGNED` | 3 | Connections from one address that haven't signed in yet |
| `CITRINE_IP_CONNECTS_PER_MINUTE` | 20 | New connections from one address per minute |
| `CITRINE_SIGN_IN_FRAME_KB` | 4 | Size of one message before sign-in; a larger one closes the connection |
| `CITRINE_WRONG_TRIES_PER_MINUTE` | 10 | Wrong relay passwords or invite codes per minute, from one address for sign-ups and unknown keys, and for each account on its own. Past it, sign-ins get "too many tries" for a minute |
| `CITRINE_UNKNOWN_TRIES_PER_MINUTE` | 60 | Sign-ins per minute from one address with a key no account has. An existing account still signs in from that address |
| `CITRINE_MESSAGES_PER_SECOND` | 30 | Messages per second from one account, all its connections together. Game play and upload data don't count |
| `CITRINE_MESSAGE_BURST` | 100 | Messages one account may send at once before that rate applies |
| `CITRINE_CHATS_PER_SECOND` | 5 | Chat messages per second from one account |
| `CITRINE_CHAT_BURST` | 30 | Chat messages one account may send at once |
| `CITRINE_CHAT_KB_PER_SECOND` | 256 | KB of chat per second from one account, with four seconds' worth at once |
| `CITRINE_FRIEND_REQUESTS_PER_HOUR` | 20 | New friend requests per hour from one account. Asking the same person again doesn't count |
| `CITRINE_FRIEND_REQUEST_BURST` | 10 | New friend requests one account may send at once |
| `CITRINE_DOWNLOAD_MB_PER_DAY` | 1024 | MB of files one account may download per day |
| `CITRINE_SESSION_KB_PER_SECOND` | 256 | KB per second each player may send in a play session |
| `CITRINE_SESSION_BURST_MB` | 12 | MB a player may send at once in a play session, enough for a game and a saved state |
| `CITRINE_MAX_INVITES` | 5 | Play invites one host may have waiting for an answer. A new invite to the same friend replaces the old one |
| `CITRINE_ROOM_MEMBERS` | 16 | People in one room, 2 to 64 |
| `CITRINE_ROOM_PLAYERS` | 8 | Members of one room who may run a game at once, 2 to 64 |
| `CITRINE_ROOM_GRACE_SECS` | 60 | Seconds a member whose connection dropped keeps their place in the room |
| `CITRINE_PASSWORD_CHECKS` | 2 | Relay password checks that run at once. Each takes about 19 MB of memory |
| `CITRINE_PASSWORD_QUEUE` | 16 | Password checks that may wait their turn. More get "too many tries" |
| `CITRINE_SIGN_UPS_PER_IP` | 3 | New accounts per hour from one address. More get "too many tries" |
| `CITRINE_SIGN_UPS_PER_HOUR` | 30 | New accounts per hour on the whole relay. More get "sign-ups are closed". Sign-ups with an invite code don't count |
| `CITRINE_FRIEND_REQUESTS_OUT` | 50 | Friend requests one account may have waiting for an answer. More get "slow down" |
| `CITRINE_FRIEND_REQUESTS_IN` | 200 | Friend requests one account may have waiting for it. More are dropped without a word; the sender still sees the request as sent |
| `CITRINE_FRIEND_REQUEST_DAYS` | 30 | Days a friend request waits for an answer before the relay removes it |

Each file counts as at least 4 KB toward storage, the smallest piece of disk it takes, and a save also counts its label.

## Data

Accounts, friends, chat history, invite codes, settings, the relay key and the two-factor keys live in
`citrine.db` in `CITRINE_DATA_DIR`,
shared files in its `files` folder. The server only stores encrypted messages and files and the
hash of each account's key, never the relay ID the keys come from. Chat messages stay until the account is deleted,
also after they arrive, so a device that signs in later gets the whole history. The history counts
toward `CITRINE_STORAGE_TOTAL_MB`, not toward a player's own storage.

Relays before 0.9.0 deleted a message once it arrived, so older chats live only on the devices that
had them. Each device signed in before the upgrade sends its chats once, encrypted for its own
account, the next time it connects. The server keeps one copy per account and gives it to that
account's other devices only, a device signed in later included.

An account can be online on several devices at once. Each one gets every chat, friend change and
invite, and the user's Manage menu lists the devices with a Sign out for each.

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
