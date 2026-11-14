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

    pub(super) fn net_taken(&mut self, stream: u32, total: u64) -> Result<(), ErrorCode> {
        self.me()?;
        self.net.taken(stream, total);
        Ok(())
    }
}
