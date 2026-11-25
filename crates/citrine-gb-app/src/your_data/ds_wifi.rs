use super::{Change, Files, Part, Store, Tally};
use crate::ds_wifi::Identity;
use crate::storage::SettingsStore;
use std::io;

const PATH: &str = "ds-wifi/nds-wifi.bin";

pub struct DsWifiPart<'a> {
    pub store: &'a SettingsStore,
    pub identity: &'a mut Option<Identity>,
}

impl DsWifiPart<'_> {
    fn mine(&self) -> Option<&Identity> {
        self.identity
            .as_ref()
            .filter(|identity| identity.connections.is_some())
    }

    fn theirs(files: &Files) -> Option<Identity> {
        Identity::from_bytes(files.get(PATH)?).filter(|identity| identity.connections.is_some())
    }
}

impl Part for DsWifiPart<'_> {
    fn kind(&self) -> Store {
        Store::DsWifi
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: usize::from(self.mine().is_some()),
        }
    }

    fn export(&mut self, out: &mut Files) {
        if let Some(identity) = self.mine() {
            out.insert(PATH.to_owned(), identity.to_bytes());
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        match (Self::theirs(files), self.mine()) {
            (None, _) => Change::default(),
            (Some(_), None) => Change {
                added: 1,
                replaced: 0,
            },
            (Some(theirs), Some(mine)) => Change {
                added: 0,
                replaced: usize::from(&theirs != mine),
            },
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let Some(theirs) = Self::theirs(files) else {
            return Ok(());
        };
        self.store.store_ds_wifi(&theirs)?;
        *self.identity = Some(theirs);
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        let fresh = Identity::random();
        self.store.store_ds_wifi(&fresh)?;
        *self.identity = Some(fresh);
        Ok(())
    }
}
