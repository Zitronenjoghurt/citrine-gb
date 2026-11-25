use super::Client;
use crate::ds_net::Rules;
use citrine_gb_net::{ErrorCode, NetAddr, ServerMsg};
use std::net::Ipv4Addr;

impl Client {
    fn net_rules(&self) -> Result<Rules, ErrorCode> {
        let account = self.me()?.id;
        if !self.app.settings.get().ds_net {
            return Err(ErrorCode::DsNetOff);
        }
        let config = &self.app.config;
        Ok(Rules {
            public_ip: config.ds_net_public_ip,
            private: config.ds_net_private,
            ports: config.limits.ds_net_ports,
            streams: config.limits.ds_net_streams,
            stun: config.ds_net_stun.clone(),
            account,
        })
    }

    pub(super) async fn net_send(
        &mut self,
        port: u16,
        to: NetAddr,
        data: Vec<u8>,
    ) -> Result<(), ErrorCode> {
        let rules = self.net_rules()?;
        self.net.send(&rules, &self.outbox, port, to, data).await;
        Ok(())
    }

    pub(super) async fn net_lease(&mut self, want: Option<[u8; 4]>) -> Result<(), ErrorCode> {
        self.net_rules()?;
        let who = self.me()?.online_id;
        let ip = self.net.lease(&self.outbox, who, want.map(Ipv4Addr::from));
        self.send(ServerMsg::NetLeased {
            ip: ip.map(|ip| ip.octets()),
        })
        .await;
        Ok(())
    }

    pub(super) async fn net_open(&mut self, stream: u32, to: NetAddr) -> Result<(), ErrorCode> {
        let rules = self.net_rules()?;
        let who = self.me()?.online_id;
        if !self.net.open(&rules, &self.outbox, stream, to, who) {
            self.send(ServerMsg::NetOpened { stream, ok: false }).await;
        }
        Ok(())
    }

    pub(super) async fn net_write(&mut self, stream: u32, data: Vec<u8>) -> Result<(), ErrorCode> {
        self.net_rules()?;
        if !self.net.write(stream, data) {
            self.send(ServerMsg::NetClosed { stream }).await;
        }
        Ok(())
    }

    pub(super) fn net_close(&mut self, stream: u32) -> Result<(), ErrorCode> {
        self.me()?;
        self.net.close(stream);
        Ok(())
    }

    pub(super) fn ds_host(
        &mut self,
        game: String,
        port: u16,
        keys: Vec<(String, String)>,
    ) -> Result<(), ErrorCode> {
        if self.net_rules().is_ok() {
            let named = format!("{game:?}, {} keys", keys.len());
            if !self.net.host(game, port, keys) {
                log::info!(
                    "relay connection {}: ds host refused: {named}",
                    self.conn_id
                );
            }
        }
        Ok(())
    }

    pub(super) fn ds_message(&mut self, to: NetAddr, data: Vec<u8>) -> Result<(), ErrorCode> {
        if let Ok(rules) = self.net_rules() {
            self.net.message(&rules, to, data);
        }
        Ok(())
    }

    pub(super) fn ds_natneg(&mut self, cookie: u32, index: u8, port: u16) -> Result<(), ErrorCode> {
        if let Ok(rules) = self.net_rules() {
            self.net.natneg(&rules, &self.outbox, cookie, index, port);
        }
        Ok(())
    }

    pub(super) fn ds_unhost(&mut self) -> Result<(), ErrorCode> {
        self.me()?;
        self.net.unhost();
        Ok(())
    }

    pub(super) async fn ds_list(&mut self, call: u64, game: String) -> Result<(), ErrorCode> {
        let hosts = match self.net_rules() {
            Ok(_) => self.net.hosts(&game),
            Err(_) => Vec::new(),
        };
        self.send(ServerMsg::DsList { call, hosts }).await;
        Ok(())
    }

    pub(super) fn net_taken(&mut self, stream: u32, total: u64) -> Result<(), ErrorCode> {
        self.me()?;
        self.net.taken(stream, total);
        Ok(())
    }
}
