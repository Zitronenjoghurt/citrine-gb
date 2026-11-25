use super::{Client, db};
use crate::App;
use crate::stats::Refused;
use citrine_gb_net::{
    ARCHIVE_PROTOCOL, ARCHIVE_SEALED_PART_BYTES, ArchiveId, ErrorCode, ServerMsg,
};
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

impl App {
    async fn archive_list(&self, account: i64) -> Option<ServerMsg> {
        match self.archive.games_for(&self.db, account).await {
            Ok(games) => Some(ServerMsg::Archive(games)),
            Err(e) => {
                log::error!("database: {e}");
                None
            }
        }
    }

    pub(crate) async fn tell_archive(&self, account: i64) {
        if !self.hub.is_online(account) {
            return;
        }
        if let Some(list) = self.archive_list(account).await {
            self.hub.send_speaking(account, ARCHIVE_PROTOCOL, list);
        }
    }

    pub(crate) async fn tell_archive_all(&self) {
        let accounts = match self.db.archive_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                log::error!("database: {e}");
                return;
            }
        };
        for account in accounts {
            self.tell_archive(account.id).await;
        }
    }
}

impl Client {
    fn speaks_archive(&self) -> bool {
        self.hello.is_some_and(|v| v >= ARCHIVE_PROTOCOL)
    }

    pub(super) async fn send_archive(&self, account: i64) -> Result<(), ErrorCode> {
        if !self.speaks_archive()
            || !self.app.archive.on()
            || !db(self.app.db.archive_has_access(account).await)?
        {
            return Ok(());
        }
        if let Some(list) = self.app.archive_list(account).await {
            self.send(list).await;
        }
        Ok(())
    }

    pub(super) fn archive_cancel(&mut self, id: ArchiveId) {
        if let Some((_, task)) = self.archive_download.take_if(|(at, _)| *at == id) {
            task.abort();
        }
    }

    pub(super) async fn archive_get(
        &mut self,
        id: ArchiveId,
        from_part: u32,
    ) -> Result<(), ErrorCode> {
        if !self.speaks_archive() {
            return Err(ErrorCode::BadMessage);
        }
        let me = self.me()?.id;
        let failed = move |error| ServerMsg::ArchiveFailed { id, error };
        if let Some((_, task)) = self.archive_download.take() {
            task.abort();
            let _ = task.await;
        }
        let opened = if db(self.app.db.archive_can_get(me, id).await)? {
            db(self.app.archive.open_game(&self.app.db, id).await)?
        } else {
            None
        };
        let Some(opened) = opened.filter(|o| from_part < o.parts) else {
            self.send(failed(ErrorCode::UnknownFile)).await;
            return Ok(());
        };
        let left = u64::from(opened.parts - from_part) * ARCHIVE_SEALED_PART_BYTES as u64;
        let rates = &self.app.rates.archive_downloads;
        let Some(guard) = self.app.archive.start_download(me) else {
            self.refused(Refused::Downloads);
            self.send(failed(ErrorCode::RateLimited)).await;
            return Ok(());
        };
        if !rates.take(me, (left as f64).min(rates.capacity())) {
            self.refused(Refused::Downloads);
            self.send(failed(ErrorCode::RateLimited)).await;
            return Ok(());
        }
        let app = self.app.clone();
        let tx = self.outbox.clone();
        let task = tokio::spawn(async move {
            let _guard = guard;
            let start = u64::from(from_part) * ARCHIVE_SEALED_PART_BYTES as u64;
            let file = tokio::fs::File::open(&opened.path).await;
            let Ok(mut file) = file else {
                tx.push(failed(ErrorCode::UnknownFile)).await;
                return;
            };
            if file.seek(SeekFrom::Start(start)).await.is_err() {
                tx.push(failed(ErrorCode::Server)).await;
                return;
            }
            if !tx
                .push(ServerMsg::ArchiveStart {
                    id,
                    key: opened.key,
                    prefix: opened.prefix,
                    parts: opened.parts,
                })
                .await
            {
                return;
            }
            for index in from_part..opened.parts {
                if !app.db.archive_can_get(me, id).await.unwrap_or(false) {
                    tx.push(failed(ErrorCode::UnknownFile)).await;
                    return;
                }
                let mut data = Vec::with_capacity(ARCHIVE_SEALED_PART_BYTES);
                let read = (&mut file)
                    .take(ARCHIVE_SEALED_PART_BYTES as u64)
                    .read_to_end(&mut data)
                    .await;
                if read.is_err() || data.is_empty() {
                    tx.push(failed(ErrorCode::Server)).await;
                    return;
                }
                if !tx.push(ServerMsg::ArchivePart { id, index, data }).await {
                    return;
                }
            }
        });
        self.archive_download = Some((id, task));
        Ok(())
    }
}
