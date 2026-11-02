use super::{Client, MAX_UPLOADS, PutSave, Upload, db, file_path, hex, quota_of, random_file_id};
use crate::db::{NewSave, SaveWrite};
use crate::now;
use crate::stats::Refused;
use crate::storage::charge;
use citrine_gb_net::{
    CHUNK_BYTES, ErrorCode, FileId, FileInfo, GameKey, MAX_LABEL_BYTES, SaveInfo, ServerMsg, Tag,
    valid_slot,
};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

impl Client {
    pub(super) async fn upload(&mut self, tag: Tag, size: u64) -> Result<(), ErrorCode> {
        if self.uploads.contains_key(&tag) {
            return Err(ErrorCode::BadMessage);
        }
        if self.uploads.len() >= MAX_UPLOADS {
            self.refused(Refused::Messages);
            return Err(ErrorCode::RateLimited);
        }
        let row = self.my_row().await?;
        let quota = quota_of(&self.app, &row);
        if quota == 0 {
            return Err(ErrorCode::SharingOff);
        }
        if size == 0 || size > self.app.settings.get().max_file_bytes {
            return Err(ErrorCode::TooLarge);
        }
        let usage = db(self.app.db.usage(row.id).await)?;
        if usage.files + self.uploads.len() as u64 >= self.app.config.limits.max_files {
            self.refused(Refused::Storage);
            return Err(ErrorCode::QuotaFull);
        }
        let cost = charge(size);
        if usage.bytes + self.reserved() + cost > quota {
            self.refused(Refused::Storage);
            return Err(ErrorCode::QuotaFull);
        }
        self.room(cost).await?;
        let reserved = self.app.storage.reserve(cost);
        let id = random_file_id();
        let path = self.app.files_dir.join(format!("{}.part", hex(&id.0)));
        let file = tokio::fs::File::create(&path).await.map_err(|e| {
            log::error!("upload: {e}");
            ErrorCode::Server
        })?;
        self.uploads.insert(
            tag,
            Upload {
                id,
                size,
                received: 0,
                file,
                path,
                reserved,
            },
        );
        self.send(ServerMsg::UploadReady(tag)).await;
        Ok(())
    }

    pub(super) fn reserved(&self) -> u64 {
        self.uploads.values().map(|u| u.reserved.bytes()).sum()
    }

    pub(super) async fn chunk(&mut self, tag: Tag, data: &[u8]) -> Result<(), ErrorCode> {
        let upload = self.uploads.get_mut(&tag).ok_or(ErrorCode::UnknownFile)?;
        if upload.received + data.len() as u64 > upload.size {
            return Err(ErrorCode::TooLarge);
        }
        let io = |e: std::io::Error| {
            log::error!("upload: {e}");
            ErrorCode::Server
        };
        upload.file.write_all(data).await.map_err(io)?;
        upload.received += data.len() as u64;
        if upload.received < upload.size {
            return Ok(());
        }
        let mut upload = self.uploads.remove(&tag).ok_or(ErrorCode::Server)?;
        upload.file.flush().await.map_err(io)?;
        upload.file.sync_all().await.map_err(io)?;
        let path = file_path(&self.app, upload.id);
        tokio::fs::rename(&upload.path, &path).await.map_err(io)?;
        let created = now();
        let me = self.me()?.id;
        if let Err(e) = db(self
            .app
            .db
            .insert_file(upload.id, me, upload.size, created)
            .await)
        {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(e);
        }
        self.send(ServerMsg::Uploaded {
            tag,
            file: FileInfo {
                id: upload.id,
                size: upload.size,
                created,
            },
        })
        .await;
        self.send_account().await
    }

    pub(super) async fn download(&mut self, file: FileId) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let (owner, info) = db(self.app.db.file(file).await)?.ok_or(ErrorCode::UnknownFile)?;
        let banned =
            owner != me && db(self.app.db.account(owner).await)?.is_none_or(|row| row.banned);
        if banned || owner != me && !db(self.app.db.are_friends(owner, me).await)? {
            return Err(ErrorCode::UnknownFile);
        }
        self.downloads.retain(|d| !d.is_finished());
        let limits = &self.app.config.limits;
        if self.downloads.len() >= limits.downloads_at_once + limits.downloads_waiting
            || !self.app.rates.downloads.take(me, info.size as f64)
        {
            self.refused(Refused::Downloads);
            return Err(ErrorCode::RateLimited);
        }
        let path = file_path(&self.app, file);
        let tx = self.outbox.clone();
        let slots = self.download_slots.clone();
        self.downloads.push(tokio::spawn(async move {
            let Ok(_slot) = slots.acquire_owned().await else {
                return;
            };
            let Ok(mut f) = tokio::fs::File::open(&path).await else {
                tx.push(ServerMsg::DownloadFailed {
                    file,
                    error: ErrorCode::UnknownFile,
                })
                .await;
                return;
            };
            if !tx
                .push(ServerMsg::FileStart {
                    file,
                    size: info.size,
                })
                .await
            {
                return;
            }
            let mut left = info.size;
            while left > 0 {
                let mut data = vec![0; CHUNK_BYTES.min(left as usize)];
                if f.read_exact(&mut data).await.is_err() {
                    tx.push(ServerMsg::DownloadFailed {
                        file,
                        error: ErrorCode::Server,
                    })
                    .await;
                    return;
                }
                left -= data.len() as u64;
                if !tx.push(ServerMsg::FileChunk { file, data }).await {
                    return;
                }
            }
        }));
        Ok(())
    }

    pub(super) async fn put_save(
        &mut self,
        tag: Tag,
        put: PutSave<'_>,
    ) -> Result<(), (ErrorCode, Option<SaveInfo>)> {
        let plain = |code: ErrorCode| (code, None);
        if !valid_slot(put.slot) || put.label.len() > MAX_LABEL_BYTES {
            return Err(plain(ErrorCode::BadMessage));
        }
        let row = self.my_row().await.map_err(plain)?;
        let quota = quota_of(&self.app, &row);
        if quota == 0 {
            return Err(plain(ErrorCode::SharingOff));
        }
        let size = put.data.len() as u64;
        if size == 0 || size > self.app.settings.get().max_file_bytes {
            return Err(plain(ErrorCode::TooLarge));
        }
        let current = db(self.app.db.save(row.id, put.game, put.slot).await).map_err(plain)?;
        let version = current.as_ref().map_or(0, |(info, _)| info.version);
        if version != put.base {
            return Err((ErrorCode::SaveChanged, current.map(|(info, _)| info)));
        }
        let usage = db(self.app.db.usage(row.id).await).map_err(plain)?;
        if current.is_none() && usage.saves >= self.app.config.limits.max_saves {
            self.refused(Refused::Storage);
            return Err(plain(ErrorCode::QuotaFull));
        }
        let replacing = current
            .as_ref()
            .map_or(0, |(info, _)| charge(info.size) + info.label.len() as u64);
        let cost = charge(size) + put.label.len() as u64;
        let used = usage.bytes + self.reserved();
        if used.saturating_sub(replacing).saturating_add(cost) > quota {
            self.refused(Refused::Storage);
            return Err(plain(ErrorCode::QuotaFull));
        }
        self.room(cost.saturating_sub(replacing))
            .await
            .map_err(plain)?;
        let id = random_file_id();
        let path = file_path(&self.app, id);
        let part = self.app.files_dir.join(format!("{}.part", hex(&id.0)));
        let io = |e: std::io::Error| {
            log::error!("save: {e}");
            plain(ErrorCode::Server)
        };
        let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
        file.write_all(put.data).await.map_err(io)?;
        file.sync_all().await.map_err(io)?;
        drop(file);
        tokio::fs::rename(&part, &path).await.map_err(io)?;
        let new = NewSave {
            game: put.game,
            slot: put.slot,
            base: put.base,
            label: put.label,
            file: id,
            size,
        };
        let written = match db(self.app.db.put_save(row.id, new, now()).await) {
            Ok(written) => written,
            Err(code) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(plain(code));
            }
        };
        match written {
            SaveWrite::Stored { info, replaced } => {
                if let Some(old) = replaced {
                    let _ = tokio::fs::remove_file(file_path(&self.app, old)).await;
                }
                self.send(ServerMsg::SaveStored { tag, info }).await;
                self.others(ServerMsg::SavesChanged).map_err(plain)?;
                self.send_account().await.map_err(plain)
            }
            SaveWrite::Changed(current) => {
                let _ = tokio::fs::remove_file(&path).await;
                Err((ErrorCode::SaveChanged, current))
            }
        }
    }

    pub(super) async fn get_save(&mut self, game: GameKey, slot: String) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let Some((info, file)) = db(self.app.db.save(me, game, &slot).await)? else {
            self.send(ServerMsg::SaveMissing { game, slot }).await;
            return Ok(());
        };
        match tokio::fs::read(file_path(&self.app, file)).await {
            Ok(data) => self.send(ServerMsg::Save { info, data }).await,
            Err(e) => {
                log::error!("save {}: {e}", hex(&file.0));
                self.send(ServerMsg::SaveMissing { game, slot }).await;
            }
        }
        Ok(())
    }
}
