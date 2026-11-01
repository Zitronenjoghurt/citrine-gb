use citrine_gb::gb::GameBoy;
use citrine_gb::netplay::PeerCable;
use citrine_gb_gamedata::{Game, Generation, RomTables};
use citrine_gb_partner::{Event, Happening, OfferError, Partner, Stage};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desk {
    CableClub,
    Trade,
    TimeCapsule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub icon: &'static str,
    pub headline: &'static str,
    pub detail: Option<String>,
}

pub fn status(stage: Stage, desk: Desk, fresh_trade: bool, name: &str) -> Status {
    let line = |icon, headline, detail: Option<String>| Status {
        icon,
        headline,
        detail,
    };
    if fresh_trade && stage != Stage::Confirming {
        return Status {
            icon: crate::icons::CHECK,
            headline: "Traded",
            detail: Some(format!("Another {name} is ready.")),
        };
    }
    match stage {
        Stage::Waiting => {
            let desk = match desk {
                Desk::CableClub => "Talk to the Cable Club desk in any Pokémon Center.",
                Desk::Trade => "Talk to the trade desk upstairs in any Pokémon Center.",
                Desk::TimeCapsule => {
                    "Talk to the Time Capsule desk upstairs in any Pokémon Center."
                }
            };
            line(
                crate::icons::HOURGLASS,
                "Waiting in the Cable Club",
                Some(desk.to_owned()),
            )
        }
        Stage::Linked => {
            let detail = match desk {
                Desk::CableClub => "Pick Trade Center, then sit at the table and press A.",
                Desk::Trade | Desk::TimeCapsule => "Sit at the table and press A.",
            };
            line(crate::icons::LINK, "Linked", Some(detail.to_owned()))
        }
        Stage::Exchanging | Stage::Choosing => line(
            crate::icons::ARROWS_LEFT_RIGHT,
            "Pick a Pokémon to trade",
            None,
        ),
        Stage::Confirming => line(crate::icons::ARROWS_LEFT_RIGHT, "Confirm the trade", None),
    }
}

pub struct PartnerLink {
    partner: Partner,
    cable: PeerCable,
    name: String,
    moves: Vec<String>,
    held: Option<String>,
    desk: Desk,
    fresh_trade: bool,
}

impl PartnerLink {
    pub fn start(
        game: Game,
        rom: &[u8],
        event: &'static Event,
        seed: u64,
    ) -> Result<Self, OfferError> {
        let tables = RomTables::load(game, rom).map_err(|_| OfferError::Rom)?;
        let name = tables
            .species_for_dex(event.dex)
            .and_then(|species| tables.species_name(species))
            .ok_or(OfferError::Species)?
            .to_owned();
        let moves = event
            .moves
            .iter()
            .filter_map(|&id| tables.move_name(id).map(str::to_owned))
            .collect();
        let held = match game.generation() {
            Generation::One => None,
            Generation::Two => event.item.and_then(|id| tables.item_name(id)),
        };
        let partner = Partner::new(game, rom, event, seed)?;
        let desk = match (game.generation(), partner.through_time_capsule()) {
            (Generation::One, _) => Desk::CableClub,
            (Generation::Two, true) => Desk::TimeCapsule,
            (Generation::Two, false) => Desk::Trade,
        };
        let mut cable = PeerCable::new();
        cable.set_plugged(true);
        Ok(Self {
            partner,
            cable,
            name,
            moves,
            held,
            desk,
            fresh_trade: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn moves(&self) -> &[String] {
        &self.moves
    }

    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    pub fn event(&self) -> &'static Event {
        self.partner.event()
    }

    pub fn partner(&self) -> &Partner {
        &self.partner
    }

    pub fn plugged(&self) -> bool {
        self.cable.plugged()
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.cable.set_plugged(plugged);
    }

    pub fn status(&self) -> Status {
        status(
            self.partner.stage(),
            self.desk,
            self.fresh_trade,
            &self.name,
        )
    }

    pub fn run_frame(&mut self, gb: &mut GameBoy) {
        self.cable.run_frame(gb, &mut self.partner);
        for happening in self.partner.take_happenings() {
            match happening {
                Happening::Traded { .. } => self.fresh_trade = true,
                Happening::Linked | Happening::Chosen { .. } | Happening::Canceled => {
                    self.fresh_trade = false;
                }
                Happening::Exchanged => {}
            }
        }
    }

    pub fn finish(mut self, gb: &mut GameBoy) {
        self.cable.unplug(gb);
    }
}
